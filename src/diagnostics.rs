use crate::config::{self, Config};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};
fn mouse_socket() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        crate::home().join("Library/Caches/lan-mouse-socket.sock")
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/unknown"))
            .join("lan-mouse-socket.sock")
    }
}
pub fn peers() -> Result<()> {
    let mut socket = UnixStream::connect(mouse_socket())?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.write_all(b"{\"Enumerate\":[]}\n")?;
    for row in BufReader::new(socket).lines() {
        let state: Value = serde_json::from_str(&row?)?;
        if let Some(clients) = state.get("Enumerate").and_then(Value::as_array) {
            let peers:Vec<_>=clients.iter().map(|c|json!({"client_id":c[0],"hostname":c[1]["hostname"],"position":c[1]["pos"],"active":c[2]["active"]})).collect();
            println!("{}", serde_json::to_string_pretty(&peers)?);
            return Ok(());
        }
    }
    bail!("LAN Mouse did not return its client list")
}
pub fn doctor() -> Result<()> {
    let config = Config::load()?;
    let mut report = json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"computer_name":config.computer_name,"target_client":config.target_client,"config_file":config::path(),"lan_mouse_running":UnixStream::connect(mouse_socket()).is_ok(),"lan_mouse_log_readable":std::fs::File::open(&config.lan_mouse_log).is_ok(),"ssh_host":config.ssh_host,"input_selection":config.input_source});
    #[cfg(target_os = "macos")]
    {
        report["microphone_permission"] = json!(crate::mac::permission());
        report["blackhole_available"] = json!(crate::mac_audio::Incoming::available());
    }
    #[cfg(target_os = "linux")]
    {
        for name in ["ssh", "pactl", "pacat", "parec"] {
            report[format!("{name}_available")] = json!(std::env::var_os("PATH")
                .is_some_and(|path| std::env::split_paths(&path).any(|p| p.join(name).is_file())));
        }
        let output = std::process::Command::new("pactl")
            .arg("get-default-source")
            .output()?;
        let source = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        report["audio_server_reachable"] = json!(output.status.success());
        report["default_input"] = json!(source);
        report["default_input_is_monitor"] = json!(source.ends_with(".monitor"));
    }
    if let Ok(state) = std::fs::read(config::state_dir().join("status.json")) {
        if let Ok(state) = serde_json::from_slice::<Value>(&state) {
            #[cfg(target_os = "macos")]
            if let Some(authorized) = state["authorized"].as_bool().filter(|_| {
                state["updated"]
                    .as_f64()
                    .is_some_and(|time| crate::now() - time < 3.0)
            }) {
                report["microphone_permission"] = json!(authorized);
            }
            report["agent_status"] = state;
        }
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
