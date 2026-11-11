mod config;
mod diagnostics;
mod focus;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod mac_audio;
#[cfg(target_os = "macos")]
mod mac_setup;
#[cfg(target_os = "macos")]
mod mac_ui;
#[cfg(any(target_os = "macos", test))]
mod pcm;
#[cfg(target_os = "linux")]
mod receiver;
#[cfg(target_os = "linux")]
mod route;
#[cfg(any(target_os = "macos", test))]
mod support;
mod wire;

use anyhow::{bail, Context, Result};
use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
};

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is required"))
}
pub fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--help")|Some("help")=>println!("Lan Mic {}\n\nCommands:\n  doctor                      Check setup without recording audio\n  peers                       List LAN Mouse client IDs\n  configure [options]         Save settings\n    --name NAME               Computer display name\n    --menu-bar-labels MODE    computer_names or platforms\n    --target-client ID        LAN Mouse peer to follow\n    --input SOURCE            Microphone source name (or default)\n    --host SSH_HOST           Peer SSH host (or none to listen)\n    --lan-mouse-log PATH      Override daemon log path\n  receiver [--host SSH_HOST]  Connect to a peer (Linux)\n  receiver --status          Show current Linux status\n  agent [--background]       Run local agent\n  show / about               Open Mac windows\n  relay [SOCKET]             Private local endpoint used over SSH\n",env!("CARGO_PKG_VERSION")),
        Some("doctor")=>diagnostics::doctor()?,
        Some("peers")=>diagnostics::peers()?,
        Some("--version") => println!("lan-mic {} (Rust)", env!("CARGO_PKG_VERSION")),
        Some("configure")=>config::Config::configure(args)?,
        Some("relay") => {
            let path = args.next().map(PathBuf::from).unwrap_or_else(|| config::state_dir().join("audio.sock"));
            let mut socket = UnixStream::connect(path).context("Connect to the Mac menu-bar app")?;
            let mut to_peer=socket.try_clone()?;
            std::thread::spawn(move||{let _=std::io::copy(&mut std::io::stdin().lock(),&mut to_peer);let _=to_peer.shutdown(std::net::Shutdown::Write);});
            let mut output = std::io::stdout().lock();
            let mut buffer = [0; 65536];
            loop {
                let count = socket.read(&mut buffer)?;
                if count == 0 { break; }
                output.write_all(&buffer[..count])?;
                // stdout is line-buffered even over SSH. Status/PCM frames
                // contain no guaranteed newline; flush every received block.
                output.flush()?;
            }
        }
        #[cfg(target_os = "linux")]
        Some("receiver") => {
            let config=config::Config::load()?;
            let mut host=config.ssh_host.clone();
            // Expand HOME on the remote machine; no username is embedded in the binary.
            let mut remote=config.remote_command.unwrap_or_else(||"if [ -x \"$HOME/Applications/Lan Mic.app/Contents/MacOS/LanMic\" ]; then exec \"$HOME/Applications/Lan Mic.app/Contents/MacOS/LanMic\" relay; else exec \"$HOME/.local/bin/lan-mic\" relay; fi".to_owned());
            let mut status = false;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--host" => host = Some(args.next().context("--host needs a hostname")?),
                    "--remote-command" => remote = args.next().context("--remote-command needs a command")?,
                    "--status" => status = true,
                    _ => bail!("Unknown argument {arg}"),
                }
            }
            tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(receiver::run(host.as_deref(), &remote, status))?;
        }
        #[cfg(target_os="linux")]
        Some("agent")=>{tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(receiver::run(None,"",false))?;}
        #[cfg(target_os = "macos")]
        Some("show") => mac_ui::request("show")?,
        #[cfg(target_os = "macos")]
        Some("about") => mac_ui::request("about")?,
        #[cfg(target_os = "macos")]
        Some("ui-status") => mac_ui::request("status")?,
        #[cfg(target_os = "macos")]
        Some("agent") => mac::run(args.any(|s| s == "--background"))?,
        #[cfg(target_os = "macos")]
        None => {
            if !mac_setup::install_if_needed()? {
                mac::run(false)?;
            }
        },
        _ => bail!("Usage: lan-mic --version | relay [socket] | receiver [--host NAME] [--status] | agent (macOS)"),
    }
    Ok(())
}
