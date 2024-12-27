![Maintenance](https://img.shields.io/badge/maintenance-as--is-yellow.svg)
![Build Status](https://github.com/mashu/qmx-capture/workflows/Rust/badge.svg)

# qmx-capture

QMX capture is a terminal based pan-adapter.

## Examples
```rust
cargo run
Available input devices:
----------------------
0. pipewire (44100 Hz)
1. pulse (44100 Hz)
2. default (44100 Hz)
3. plughw:CARD=Transceiver,DEV=0 (44100 Hz)

Select device number (0-3): 
3
Selected device: plughw:CARD=Transceiver,DEV=0 @ 44100 Hz
Press Enter to start visualization...
```

Current version: 0.1.0

License: MIT
