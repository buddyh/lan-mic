# How Lan Mic works

One peer opens SSH and runs the other peer's `relay` command. That command joins
stdin/stdout to a user-only Unix socket owned by the local Lan Mic agent. The one
connection is full duplex. On a Mac/Linux pair Linux connects; either Linux host
can connect in a Linux/Linux pair. One selected LAN Mouse client is followed.

```text
LAN Mouse log → confirmed focus → local capture
                                      ↓
                            framed PCM + status
                                      ↓
                           SSH ↔ local relay
                                      ↓
                           received virtual input
```

`focus.rs` follows an entering-client/acknowledgement pair, resets on releases,
disconnects and log rotation, and skips historical log contents at startup.
`wire.rs` decodes a one-byte kind plus four-byte big-endian payload length. `S`
contains protocol-1 JSON status; `A` contains even-length mono s16le PCM at 48 kHz.
Payloads are capped at 65,536 bytes. Status announces capture before its first PCM.
No handshake or audio data is interpreted as a shell command.

On Linux, `receiver.rs` manages both roles through `parec` and `pacat`; `route.rs`
creates a virtual input and preserves defaults and recording streams. On Mac,
`mac.rs` runs CPAL capture/resampling on a dedicated audio-management thread,
`mac_audio.rs` plays received PCM into BlackHole 2ch and saves/restores the default
input, and `mac_ui.rs` owns native AppKit windows on the main thread.

Only real received PCM activates the input route. Returning focus, mute on the
Mac, peer loss, audio timeout, or normal shutdown stops forwarding. Saved route
state allows recovery after a crash. A user's manual default-device change takes
precedence over restoration. Audio queues are bounded; speaker-monitor and received
inputs are rejected as local send sources to reduce feedback.

The transport is SSH/TCP. There is no adaptive UDP jitter buffer, codec compression,
multi-peer arbitration, Windows backend, or official LAN Mouse focus-event API yet.
Replacing the log adapter with upstream focus events is a useful future contribution.
