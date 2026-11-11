# Contributing to Lan Mic

Thanks for helping make microphone handoff reliable. This is a development preview;
small, focused changes and clear hardware reports are especially useful.

## Build and check

Use Rust 1.90 or newer; CI uses 1.95.0. See README.md for platform prerequisites.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

Mac code must also be checked on a native Mac. The optional Linux live audio test
is described in the README; it changes audio routes temporarily and should be run
with recording applications closed. Do not run it in shared hosted CI.

## Pull requests

Describe the trigger, resulting behavior, and what you tested. Add a regression
test for routing, framing, focus, or recovery changes when feasible. Update setup
instructions when installation or configuration changes. Keep audio and AppKit
event handling separate, keep queues bounded, and preserve user-selected devices.

Do not include audio recordings, keys, certificates, real configuration files,
hostnames, IP addresses, usernames, or absolute paths from your computer. Use
fictional names and redact diagnostic output. Do not claim hardware coverage from
synthetic tests alone. Report security issues using SECURITY.md.

Contributions are under GPL-3.0-or-later, the same license as the project.
Contributors keep ownership of their contributions; no copyright assignment is
requested. Do not copy code with incompatible license terms.
