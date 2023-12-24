# Midi Sampler

This Rust project is a MIDI sampler that allows you to play samples based on MIDI notes. It takes a JSON configuration file "samples.json" as input, which specifies the relative path to the sample file and the MIDI note to play that sample.

## Features

- Loads a JSON configuration file to specify sample files and their corresponding MIDI notes
- Uses [Jack](https://jackaudio.org/) to handle audio connections and routing

## Sample Configuration File

```json
{
  "samples": [
    {
      "path": "samples/kick.wav",
      "note": 36
    },
    {
      "path": "samples/snare.flac",
      "note": 38
    },
    {
      "path": "samples/hihat.wav",
      "note": 42
    }
  ]
}
```

In the above example, we have three samples specified in the "samples" array. Each sample has a "path" attribute which specifies the relative path to the sample file, and a "note" attribute which indicates the MIDI note to play that sample.

## Getting Started

To build and run the project, make sure you have Rust installed on your machine and then follow these steps:

1. Clone the repository: `git clone https://github.com/your-username/midi_sampler.git`
2. Change into the project directory: `cd midi_sampler`
3. Build the project: `cargo build`
4. Run the project: `cargo run`

Make sure to place your sample files in the appropriate location specified in the JSON configuration file.

