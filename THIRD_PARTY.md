# Third-party components

Lan Mic is GPL-3.0-or-later. Rust dependencies retain their own copyrights and
licenses. Cargo.lock pins exact versions; notices in THIRD_PARTY_LICENSES accompany
binary packages. System frameworks, SSH, PulseAudio/PipeWire tools, LAN Mouse, and
BlackHole are installed separately, not bundled with this application.

| Component | Purpose | Declared license |
|---|---|---|
| anyhow | Error handling | MIT OR Apache-2.0 |
| serde / serde_json | Configuration and status | MIT OR Apache-2.0 |
| libc | Native operating-system calls | MIT OR Apache-2.0 |
| rubato | Sample-rate conversion | MIT |
| tokio | Linux async transport and processes | MIT |
| cpal | Native Mac capture/playback | Apache-2.0 |
| tray-icon / muda | Mac status menu | MIT OR Apache-2.0 |
| objc2 / block2 | Objective-C and block bindings | MIT |
| objc2 framework bindings | AppKit and AVFoundation | Zlib OR Apache-2.0 OR MIT |

The generated notice inventory also includes transitive dependencies. Public
upstream copyright names and addresses in required legal notices are intentional;
they are not the developer's private machine or account information.
