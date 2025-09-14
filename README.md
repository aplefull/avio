# Avio

A video player written in Rust using FFmpeg for media decoding and eGUI for the interface. Work in progress.

## Usage

Run without arguments to open file picker:
```
cargo run
```

Run with video file:
```
cargo run path/to/video.mp4
```

## Requirements

- Rust toolchain
- FFmpeg development libraries installed on system

## Supported Formats

Any format supported by FFmpeg (MP4, AVI, MKV, MOV, WebM, etc.)

## Building

```
cargo build --release
```