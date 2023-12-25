use jack::{Client, ClosureProcessHandler, Control};
use midir::{MidiInput, MidiInputConnection};
use serde::Deserialize;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

const NUM_CLIENT: usize = 3;
/// Each sample is described by a path to an audio file and a MIDI
/// note
#[derive(Debug, Deserialize)]
struct SampleDescr {
    path: String,
    note: u8,
}

/// The programme is initialised with a JSON representation of this
#[derive(Debug, Deserialize)]
struct Config {
    samples_descr: Vec<SampleDescr>,
}

/// Each sample is converted to a `Vec<32>` buffer and a MIDI note on
/// start up.  When the MIDI note is received the buffer is played on
/// the output
struct SampleData {
    data: Vec<f32>,
    note: u8,
}

///
fn process_samples_json(
    file_path: &str,
) -> Result<Vec<SampleDescr>, Box<dyn std::error::Error>> {
    // Read the JSON file
    let mut contents = String::new();
    let mut file = File::open(file_path)?;
    file.read_to_string(&mut contents)
        .expect("Failed to read file");
   
    // Convert JSON
    let config: Config = serde_json::from_str(&contents)?;
    
    Ok(config.samples_descr)
}

fn play_sample(sample: &[f32], sender: &Sender<f32>) {
    for f in sample {
        match sender.send(*f) {
            Ok(_) => (),
            Err(err) => panic!("{err}: Error sending: {f:?}"),
        };
    }
}

fn main() {
    // Get command line arguments.
    let args: Vec<String> = env::args().collect();
    let samples_descr: Vec<SampleDescr> =
        match process_samples_json(args[1].as_str()) {
            Ok(sd) => sd,
            Err(err) => panic!("{err}: Failed to process input"),
        };

    // Prepare the sample buffers
    let mut sample_data: Vec<SampleData> = vec![];
    for SampleDescr { path, note } in samples_descr {
        // Create a media source. Note that the MediaSource trait is
        // automatically implemented for File, among other types.
        let file = Box::new(File::open(Path::new(path.as_str())).unwrap());

        // Create the media source stream using the boxed media source from above.
        let mss = MediaSourceStream::new(file, Default::default());

        // Create a hint to help the format registry guess what format
        // reader is appropriate. In this example we'll leave it empty.
        let hint = Hint::new();

        // Use the default options when reading and decoding.
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();

        // Probe the media source stream for a format.
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &format_opts, &metadata_opts)
            .unwrap();

        // Get the format reader yielded by the probe operation.
        let mut format = probed.format;

        // Get the default track.
        let track: &Track = format.default_track().unwrap();

        // Create a decoder for the track.
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .unwrap();

        // Store the track identifier, we'll use it to filter packets.
        let track_id = track.id;

        let mut sample_count = 0;
        let mut sample_buf: Option<SampleBuffer<f32>> = None;
        let mut data: Vec<f32> = vec![];

        loop {
            // Get the next packet from the format reader.
            if let Ok(packet) = format.next_packet() {
                // If the packet does not belong to the selected track, skip it.
                if packet.track_id() != track_id {
                    continue;
                }

                // Decode the packet into audio samples, ignoring any decode errors.
                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        // The decoded audio samples may now be accessed via
                        // the audio buffer if per-channel slices of samples
                        // in their native decoded format is
                        // desired. Use-cases where the samples need to be
                        // accessed in an interleaved order or converted into
                        // another sample format, or a byte buffer is
                        // required, are covered by copying the audio buffer
                        // into a sample buffer or raw sample buffer,
                        // respectively. In the example below, we will copy
                        // the audio buffer into a sample buffer in an
                        // interleaved order while also converting to a f32
                        // sample format.

                        // If this is the *first* decoded packet, create a
                        // sample buffer matching the decoded audio buffer
                        // format.
                        if sample_buf.is_none() {
                            // Get the audio buffer specification.
                            let spec: SignalSpec = *audio_buf.spec();

                            // Get the capacity of the decoded buffer. Note:
                            // This is capacity, not length!
                            let duration = audio_buf.capacity() as u64;

                            // Create the f32 sample buffer.
                            sample_buf =
                                Some(SampleBuffer::<f32>::new(duration, spec));
                        }

                        // Copy the decoded audio buffer into the sample
                        // buffer in an interleaved format.
                        if let Some(buf) = &mut sample_buf {
                            buf.copy_interleaved_ref(audio_buf);

                            // The samples may now be access via the `samples()` function.
                            sample_count += buf.samples().len();
                            data.append(&mut buf.samples().to_vec());
                            // print!("\rDecoded {} samples", sample_count);
                        }
                    }
                    Err(Error::DecodeError(_)) => (),
                    Err(_) => break,
                }
                // println!("size() {}", data.len());
                continue;
            }
            break;
        }
        eprintln!("Total size() {sample_count}");
        sample_data.push(SampleData { data, note });
    }

    // Create a set of audio sinks for playing samples through.
    // Playing of samples rotates through the sinks
    // Make NUM_CLIENT big enough  so samples do not collide.
    // struct AudioSink {
    // 	port:jack::Port<AudioOut>,
    // 	rx:Receiver<f32>,
    // }
    // let mut audio_sinks :Vec<AudioSink> = vec![];
    let mut senders: Vec<Sender<f32>> = Vec::new();
    let mut receivers: Vec<Arc<Mutex<Receiver<f32>>>> = Vec::new();
    let mut clients = Vec::new();
    for i in 0..NUM_CLIENT {
        let (client, _status) = Client::new(
            "midi_sample_qzt",
            jack::ClientOptions::NO_START_SERVER,
        )
        .unwrap();

        let (sx, rx) = channel();

        let rx_arc = Arc::new(Mutex::new(rx));
        let rx_arc2 = rx_arc.clone();

        let mut port = //: jack::Port<jack::AudioOut> =
            client.register_port("output", jack::AudioOut); //.unwrap();
                                                            // Activate the Jack client and start the audio processing thread
        clients.push(
            client
                .activate_async(
                    (),
                    ClosureProcessHandler::new(
                        move |_c: &Client,
                              ps: &jack::ProcessScope|
                              -> Control {
                            // let mut audio_out:Result<jack::Port<jack::AudioOut>, jack::Error>
                            let output =
                                port.as_mut().unwrap().as_mut_slice(ps);

                            // Here you can process the audio data or write your
                            // custom audio generator function For example, let's
                            // generate a simple sine wave

                            // let sample_rate = c.sample_rate() as f32;
                            // let freq = 440.0; // Frequency of the sine wave
                            // let amplitude = 0.5; // Amplitude of the sine wave

                            for (_frame, sample) in
                                output.iter_mut().enumerate()
                            {
                                if let Ok(f) =
                                    rx_arc2.lock().unwrap().try_recv()
                                {
                                    *sample = f;
                                }
                            }
                            Control::Continue
                        },
                    ),
                )
                .unwrap(),
        );

        // audio_sinks.push(AudioSink{port, rx});
        eprintln!("Push sender {}", senders.len());
        senders.push(sx.clone());
        receivers.push(rx_arc.clone());
        let f: f32 = 0.3201;
        match sx.send(f) {
            Ok(_) => eprintln!("Sent {f}"),
            Err(err) => panic!("{err}: Sending to new channel, first time"),
        };
    }

    // Check senders
    for s in senders.iter() {
        let s = s.clone();
        match s.send(0.3201) {
            Ok(_) => (),
            Err(err) => panic!("{err}: Sending to new channel"),
        };
    }
    // Create a virtual midi port to read in data
    let lpx_midi = MidiInput::new("MidiSampleQzt").unwrap();
    let in_ports = lpx_midi.ports();
    let in_port = in_ports.get(0).ok_or("no input port available").unwrap();

    // // Create the channel that the buf reading closure uses to send data
    // let (sender, receiver) = channel::<f32>();

    // Index the clousre below maintains for output clients
    let mut idx = 0;
    let _conn_in: MidiInputConnection<()> = lpx_midi
        .connect(
            in_port,
            "midi_input",
            move |_stamp, message: &[u8], _| {
                // let message = MidiMessage::from_bytes(message.to_vec());
                if message.len() == 3 && message[0] == 144 {
                    // All MIDI notes from LPX start with 144, for initial
                    // noteon and noteoff
                    let velocity = message[2];
                    if velocity != 0 {
                        // NoteOn
                        eprintln!("Got note: {message:?}");
                        if let Some(sample) =
                            sample_data.iter().find(|s| s.note == message[1])
                        {
                            eprintln!("Sending on channel: {idx}");
                            play_sample(
                                &sample.data,
                                senders.get(idx).unwrap(),
                            );
                            idx += 1;
                            idx %= senders.len();
                        }
                    }
                }
            },
            (),
        )
        .unwrap();
    // Wait for the user to press enter to exit
    println!("Press enter to exit...");
    let _ = std::io::stdin().read_line(&mut String::new());
    // Deactivate the Jack client and stop the audio processing thread
    for c in clients {
        c.deactivate().unwrap();
    }
}
