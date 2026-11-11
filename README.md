# Lan Mic

Your microphone follows your keyboard and mouse between computers.

Lan Mic is a Rust companion to [LAN Mouse](https://github.com/feschber/lan-mouse).
Move to the other computer: its applications can use your microphone. Return,
mute, or disconnect: forwarding stops and the previous recording input is restored.

**Development preview, version 0.3.0.** Mac → Linux has been used on the development
pair. Linux ↔ Linux passed real-audio tests with two agents on one Linux host.
Mac receiving detects BlackHole after a restart; end-to-end Linux → Mac audio
validation has not passed yet; treat that direction as experimental. See [validation](VALIDATION.txt) for precise evidence.
Windows is not supported. This is not an official LAN Mouse release.

## What you get

- A native Mac window, Dock entry, menu-bar controls, and About window.
- Live microphone, connection, LAN Mouse, and permission status.
- A saved **Menu bar labels** preference: **Mac / Linux** or **Computer names**.
  It shows the direction too: `Mac → Linux`, `Linux → Mac`, or `Office → Studio`.
- Sending and receiving on Linux; one SSH connection carries both directions.
- Automatic microphone defaults and recording-stream restoration on Linux.
- Automatic Mac input selection/restoration using BlackHole 2ch for receiving.
- Muting on return, disconnect, stalled audio, or missing peer heartbeats.
- Configurable computer names, LAN Mouse client ID, and microphone input.

Audio stays between your computers. There is no cloud relay and Lan Mic does not
record audio files. LAN Mouse provides the **handoff state**; audio uses a
**separate authenticated SSH connection**, rather than LAN Mouse's transport.

## Install a prebuilt release

Release packages are built automatically from tagged source and published under
[GitHub Releases](https://github.com/buddyh/lan-mic/releases). The first release is
being prepared; source builds remain available below. **No Rust installation is
needed for a prebuilt package.** Check the release’s SHA-256 checksum before use.

**Mac:** choose `lan-mic-macos-arm64.zip` for Apple Silicon or
`lan-mic-macos-intel.zip` for Intel. Unzip and open **Lan Mic.app**. Its first-launch
setup installs it under your user’s Applications folder and configures login
startup. You can cancel setup. LAN Mouse must already be installed and paired.
Then allow the normal Microphone prompt and complete SSH pairing below.
To receive on a Mac, also install BlackHole 2ch and restart if requested.

The preview app is ad-hoc signed, not Developer ID signed/notarized. A downloaded
copy may be blocked by Gatekeeper and require Apple’s normal **Privacy & Security
→ Open Anyway** approval. Do not disable Gatekeeper or remove system protections.
Developer ID notarization remains a distribution improvement for a future release.

**Linux:** download the archive matching your CPU, extract it, then install as your
desktop user. For a Mac/Linux pair the Linux host opens SSH:

```sh
tar -xzf lan-mic-linux-x86_64.tar.gz
cd lan-mic-linux-x86_64
./install-linux.sh "$PWD/lan-mic" --host mic-peer
```

The virtual audio and SSH prerequisites below still apply. For two Linux machines,
use `--listen` on one and `--host mic-peer` on the other. Current packaged builds
include Linux x86_64; the script also supports native arm64 builds.

## Before installing

1. Install and pair LAN Mouse. Confirm keyboard/mouse handoff works both ways.
   Use compatible LAN Mouse builds on both computers. The newer October 2026
   development protocol does not interoperate with the June 2026 build despite
   both displaying `0.11.0`.
2. On Linux, use PipeWire's PulseAudio compatibility service (or PulseAudio),
   with `pactl`, `pacat`, and `parec` installed. Also install OpenSSH client tools.
   Run Lan Mic inside the logged-in desktop session, not as root.
3. On the SSH receiving computer, enable its normal SSH server. On a Mac this is
   **System Settings → General → Sharing → Remote Login**. Limit access to the
   intended user. The connecting Linux computer needs passwordless key login.
4. To **receive** microphone audio on a Mac, install
   [BlackHole 2ch](https://github.com/ExistentialAudio/BlackHole) and restart if its
   installer requires it. With Homebrew: `brew install --cask blackhole-2ch`.
   Sending from a Mac does not need BlackHole. macOS 14 or newer is the target;
   the installed development system is macOS 27.

Lan Mic does not change SSH trust or automatically enable a remote-login service.
First connect interactively, verify the server fingerprint, and configure a key:

```sh
ssh mic-peer true
ssh -oBatchMode=yes -oStrictHostKeyChecking=yes mic-peer true
```

`mic-peer` can be an existing SSH alias, `user@hostname`, or a Tailscale address.
The second command must succeed without asking for a password or new host trust.
If you need a new key, create it under a new filename rather than overwriting an
existing key, install its **public** key on the peer, and set `IdentityFile` and
`IdentitiesOnly yes` for that alias in `~/.ssh/config`. Do not share private keys.

## Build

Install Rust **1.90 or newer** and the platform’s compiler/linker tools. The tested
toolchain is 1.95.0. From this source directory:

```sh
./scripts/build-release.sh
cargo test --locked
```

The executable is `target/release/lan-mic`. No Python or Swift runtime is used by
the app. The optional live system-test harness uses Python's standard library.

## Mac ↔ Linux setup

The Mac runs the local app. Linux opens the single SSH connection; both can send
and receive over it. A reverse SSH key is not needed.

**On the Mac:**

```sh
./install-mac.sh "$PWD/target/release/lan-mic"
open "$HOME/Applications/Lan Mic.app"
```

Allow the app's normal **Microphone** prompt. The installer preserves an existing
bundle ID for a source build, backs up the previous app and LaunchAgents, signs the local build,
and runs the existing LAN Mouse daemon with handoff logging. LAN Mouse's own
configuration, certificate, and Accessibility permissions are kept separate.
Source-built previews are not Developer ID notarized releases; macOS may
require its normal Open/Privacy approval. No security setting is bypassed.

**On Linux:**

```sh
./install-linux.sh "$PWD/target/release/lan-mic" --host mic-peer
"$HOME/.local/share/lan-mic/lan-mic" doctor
```

The binary is also linked at `~/.local/bin/lan-mic`. If that directory is not on
your PATH, use the full path in the commands below.

The installer adds or reuses a `lan-mouse.service` user unit and adds a logging
drop-in without replacing existing capture-backend arguments. If LAN Mouse is
running outside that service, close it once and rerun the installer. Its GUI can
be opened again afterward. Both services start with the desktop session.

If the Mac installed LAN Mouse somewhere unusual, pass its executable explicitly:

```sh
LAN_MOUSE_BINARY="/custom/Lan Mouse.app/Contents/MacOS/lan-mouse" \
  ./install-mac.sh "$PWD/target/release/lan-mic"
```

## Linux ↔ Linux setup

Install the same build on both. Choose **one listener** and **one connector**.
Both roles send and receive; these names describe who opens SSH, not audio direction.

```sh
# Listener computer, whose SSH server the other computer can reach:
./install-linux.sh "$PWD/target/release/lan-mic" --listen

# Connector computer:
./install-linux.sh "$PWD/target/release/lan-mic" --host mic-peer
```

Do not create a second SSH connection for the reverse audio direction.
Run `lan-mic doctor` on both, then test a real pointer crossing in each direction.
The current end-to-end Linux test uses two agents on one machine; testing separate
Linux computers/compositors is still required before claiming broad compatibility.

## Pick the correct LAN Mouse peer and microphone

The default follows LAN Mouse **client 0**. If you have several clients, inspect
IDs and select the one connected to the other Lan Mic computer:

```sh
# Linux binary; on Mac use ~/Applications/Lan Mic.app/Contents/MacOS/LanMic
lan-mic peers
lan-mic configure --target-client 1
lan-mic configure --name "Office computer"
```

Names default to the Mac's Computer Name and Linux's hostname. The peer announces
its display name over the authenticated connection. `--peer-name "Studio"` can
override that label locally. The Mac window's **Menu bar labels** selector takes
effect immediately and persists across restarts.

On Linux, inspect microphone source names with `pactl list short sources`:

```sh
lan-mic configure --input alsa_input.usb-YOUR_MICROPHONE
# Or return to the system default:
lan-mic configure --input default
```

On Mac, `--input` accepts the CPAL device ID or exact microphone device name.
Other settings take effect when the app/service restarts. Linux speaker-monitor
sources and Lan Mic's received virtual source are rejected as sending inputs to
avoid accidentally forwarding desktop sound or creating an audio feedback loop.
A Linux machine with no local microphone can still receive Mac microphone audio.

## Check that it works

1. Open the Mac window or run `lan-mic receiver --status` on Linux.
2. Move the pointer into the other computer. Its input should change only after
   a confirmed handoff and actual audio arrival.
3. Speak and check the target application's input meter. Linux exposes `lan_mic`
   (the label is `LAN_Microphone`, or `Mac_Microphone` on older installations).
   Mac receiving uses `BlackHole 2ch`.
4. Return the pointer. Forwarding should stop and the previous recording default
   should return. Check Mute as well, then disconnect once and verify it silences.

Apps that explicitly choose another microphone may ignore a system default.
Choose the virtual microphone in that app when needed. Mac per-application
recording routes are not forcibly changed; some apps require selecting BlackHole
or reopening their audio settings. On Linux, streams on the former default move
with the handoff, including recording streams opened afterward. Each stream is
moved once; later manual stream/default changes are preserved.

Closing the Mac window leaves forwarding running. Reopen it from the Dock,
Finder, **Settings…** in the menu bar, or the app's Window menu. **Quit Lan Mic**
ends capture and the local connection; Linux restores its routes on disconnect.

## Troubleshooting

```sh
lan-mic doctor
lan-mic peers
systemctl --user status lan-mic lan-mouse
journalctl --user -u lan-mic -n 50
```

On Mac, use the window's **Microphone settings…**, **Open diagnostics**, and
**About Lan Mic** controls. Logs are in `~/Library/Logs/LanMic/`.

| Symptom | What to check |
|---|---|
| Connected but never forwards | Correct `target_client`, readable daemon log, and fresh pointer crossing after helper restart. |
| Microphone permission required | Allow Lan Mic under Privacy & Security → Microphone. Rebuilt ad-hoc-signed apps may need permission renewed. |
| Mac says it needs BlackHole/restart | Complete the driver install and restart; merely copying its driver file is insufficient. |
| Linux says choose a local microphone | The default is a speaker monitor/received source, or no microphone is connected. Select a physical source. |
| SSH fails or keeps reconnecting | The exact BatchMode/StrictHostKeyChecking command above must succeed under the desktop user. |
| LAN Mouse stops working after an upgrade | Match compatible LAN Mouse versions; verify its own capture/emulation backends and Accessibility permission. |
| An app still uses its old microphone | Check that application's explicit input selection. |
| Previous microphone was unplugged | Select an available local input; Lan Mic never guesses an unrelated device to restore. |
| Another instance is running | Run one agent per user and one SSH connection per pair; check the user service. |

Only the selected LAN Mouse peer is followed. Restarting the helper does not replay
old logs or assume remote focus; move out and back once to establish a fresh handoff.

## Updates and uninstall

Rebuild the same source version on both computers and rerun the installers.
Backups are printed at installation. System package managers do not update these
locally built binaries. Review changes and verify a crossing after updates.

```sh
./uninstall-linux.sh
# On the Mac:
./uninstall-mac.sh
```

Settings and backups are retained. The Linux virtual source stays silent until the
next audio-server restart. The Mac uninstaller leaves BlackHole installed because
other apps may use it, and reopens LAN Mouse normally.

## Reliability and development

- Capture waits for an entering-client message followed by its acknowledgement.
  Failed edges, duplicate/late ACKs, release, and daemon restart are covered.
- Audio/status framing is bounded; partial frames survive arbitrary read boundaries.
- Audio callbacks have bounded queues. Old/stalled audio fails closed.
- Status heartbeats, audio timeouts, player exit, process shutdown, and connection
  loss end forwarding. Persisted route state supports recovery after a crash.
- Manual default-device changes are respected during restoration.
- Mac audio runs separately from AppKit event handling, so menus and window dragging
  do not block the audio engine.
- SSH uses existing host trust and keys; local sockets are user-only. No new public
  audio port is opened. PCM is mono, signed 16-bit, 48 kHz, about 0.77 Mbit/s per
  active direction before SSH overhead.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
# Opt-in live PipeWire test; close recording apps first:
python3 tests/system_linux.py --binary "$PWD/target/release/lan-mic"
```

The live test pauses/restores `lan-mic.service`, uses generated audio and private
fixture logs, and verifies Linux ↔ Linux audio/default restoration/disconnect.
It never edits LAN Mouse's real log or records a real microphone. CI configuration
runs Rust checks on Linux and Mac; it is supplied but has not run on a hosted CI
service yet. Native hardware and OS permissions cannot be certified by unit tests.

The adapter currently follows LAN Mouse logs because its public IPC lacks a complete
focus-change event stream. A future upstream integration should replace this with
explicit focus events. SSH/TCP is used, without adaptive UDP jitter buffering.
Multi-peer arbitration, Windows support, an external user test matrix, long-running
soak tests, and signed/notarized releases remain release work.
Release packaging creates a draft prerelease for review;
see [release preparation](docs/RELEASING.md).

See [architecture](docs/ARCHITECTURE.md), [contributing](CONTRIBUTING.md),
[security and privacy](SECURITY.md), and the exact [validation record](VALIDATION.txt).
The source package contains no retired prototype or development-machine state.

## Author and license

Created by **Buddy Hadry**.
Licensed under **GPL-3.0-or-later**; see [LICENSE](LICENSE). There is no warranty.
Dependencies retain their own licenses; see [third-party notices](THIRD_PARTY.md).
LAN Mouse and BlackHole are separate projects and are not bundled here.

The Mac menu contains status, Settings, mute and quit. Settings contains the display
preference, microphone-access controls, diagnostics, and About. About includes the
author, license, and a GitHub link. A one-time “Like LAN Mic? Star us on Github!”
prompt can appear after three completed handoffs and ten minutes of forwarding,
while Settings is foregrounded and forwarding is idle. Closing or dismissing it
prevents future automatic prompts. Usage counters stay on your computer; no
analytics or GitHub account checks are sent.
