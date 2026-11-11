use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub menu_bar_labels: MenuBarLabels,
    pub computer_name: String,
    pub peer_name: Option<String>,
    pub target_client: u64,
    pub input_source: Option<String>,
    pub ssh_host: Option<String>,
    pub remote_command: Option<String>,
    pub lan_mouse_log: PathBuf,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MenuBarLabels {
    #[default]
    ComputerNames,
    Platforms,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            menu_bar_labels: MenuBarLabels::ComputerNames,
            computer_name: system_name(),
            peer_name: None,
            target_client: 0,
            input_source: None,
            ssh_host: None,
            remote_command: None,
            lan_mouse_log: default_log(),
        }
    }
}
pub fn state_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        crate::home().join("Library/Application Support/Lan Mic")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::home().join(".local/state"))
            .join("lan-mic")
    }
}
pub fn path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        state_dir().join("config.json")
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::home().join(".config"))
            .join("lan-mic/config.json")
    }
}
fn default_log() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        crate::home().join("Library/Logs/LanMic/lan-mouse.log")
    }
    #[cfg(not(target_os = "macos"))]
    {
        state_dir().join("lan-mouse.log")
    }
}
fn system_name() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !s.is_empty() {
                return clean_name(&s);
            }
        }
    }
    let mut buffer = [0u8; 256];
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0 {
        let n = buffer.iter().position(|b| *b == 0).unwrap_or(buffer.len());
        return clean_name(&String::from_utf8_lossy(&buffer[..n]));
    }
    "This computer".into()
}
pub fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_owned()
}
#[cfg(any(target_os = "macos", test))]
pub enum DisplayState {
    Idle,
    Muted,
    Sending,
    Receiving,
}
#[cfg(any(target_os = "macos", test))]
pub fn tray_title(
    style: MenuBarLabels,
    local: &str,
    peer: &str,
    local_platform: &str,
    peer_platform: &str,
    state: DisplayState,
) -> String {
    fn platform(p: &str) -> &str {
        match p {
            "macos" => "Mac",
            "linux" => "Linux",
            _ => "Computer",
        }
    }
    fn compact(name: &str) -> String {
        let mut chars = name.chars();
        let short: String = chars.by_ref().take(24).collect();
        if chars.next().is_some() {
            format!("{short}…")
        } else {
            short
        }
    }
    let (local, peer) = if style == MenuBarLabels::Platforms {
        (
            platform(local_platform).to_owned(),
            platform(peer_platform).to_owned(),
        )
    } else {
        (compact(local), compact(peer))
    };
    match state {
        DisplayState::Idle => format!("🎙 {local}"),
        DisplayState::Muted => format!("🎙 {local} · Muted"),
        DisplayState::Sending => format!("🎙 {local} → {peer}"),
        DisplayState::Receiving => format!("🎙 {peer} → {local}"),
    }
}
impl Config {
    pub fn load() -> Result<Self> {
        let p = path();
        if p.exists() {
            serde_json::from_slice(&std::fs::read(&p)?)
                .with_context(|| format!("Read {}", p.display()))
        } else {
            Ok(Self::default())
        }
    }
    pub fn save(&self) -> Result<()> {
        if clean_name(&self.computer_name).is_empty() {
            bail!("Computer name cannot be empty");
        }
        let p = path();
        std::fs::create_dir_all(p.parent().unwrap())?;
        crate::atomic_json(&p, self)
    }
    pub fn configure(mut args: impl Iterator<Item = String>) -> Result<()> {
        let mut config = Self::load()?;
        while let Some(arg) = args.next() {
            let value = args
                .next()
                .context("Configuration options require a value")?;
            match arg.as_str() {
                "--name" => config.computer_name = clean_name(&value),
                "--menu-bar-labels" => {
                    config.menu_bar_labels = match value.as_str() {
                        "computer_names" => MenuBarLabels::ComputerNames,
                        "platforms" => MenuBarLabels::Platforms,
                        _ => bail!("Use computer_names or platforms for --menu-bar-labels"),
                    }
                }
                "--peer-name" => config.peer_name = Some(clean_name(&value)),
                "--target-client" => config.target_client = value.parse()?,
                "--input" => {
                    config.input_source = if value == "default" {
                        None
                    } else {
                        Some(value)
                    }
                }
                "--host" => config.ssh_host = if value == "none" { None } else { Some(value) },
                "--lan-mouse-log" => config.lan_mouse_log = value.into(),
                "--remote-command" => config.remote_command = Some(value),
                _ => bail!("Unknown configuration option: {arg}"),
            }
        }
        config.save()?;
        println!("Saved {}. Restart Lan Mic to apply.", path().display());
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_bounded_without_control_characters() {
        assert_eq!(clean_name("Office\nMac\u{1b}"), "OfficeMac");
        assert_eq!(clean_name(&"x".repeat(100)).len(), 64);
    }
    #[test]
    fn rejects_unknown_fields_and_defaults_new_fields() {
        let c: Config =
            serde_json::from_str(r#"{"computer_name":"Workstation","target_client":3}"#).unwrap();
        assert_eq!(c.target_client, 3);
        assert!(serde_json::from_str::<Config>(r#"{"ssh_hots":"typo"}"#).is_err());
    }
    #[test]
    fn label_preference_covers_both_directions_and_persists() {
        assert_eq!(
            tray_title(
                MenuBarLabels::Platforms,
                "Office",
                "Desk",
                "macos",
                "linux",
                DisplayState::Sending
            ),
            "🎙 Mac → Linux"
        );
        assert_eq!(
            tray_title(
                MenuBarLabels::Platforms,
                "Office",
                "Desk",
                "macos",
                "linux",
                DisplayState::Receiving
            ),
            "🎙 Linux → Mac"
        );
        assert_eq!(
            tray_title(
                MenuBarLabels::ComputerNames,
                "Office",
                "Desk",
                "macos",
                "linux",
                DisplayState::Sending
            ),
            "🎙 Office → Desk"
        );
        assert_eq!(
            tray_title(
                MenuBarLabels::Platforms,
                "Office",
                "Desk",
                "macos",
                "linux",
                DisplayState::Idle
            ),
            "🎙 Mac"
        );
        assert_eq!(
            tray_title(
                MenuBarLabels::Platforms,
                "Office",
                "Desk",
                "macos",
                "linux",
                DisplayState::Muted
            ),
            "🎙 Mac · Muted"
        );
        let c = Config {
            menu_bar_labels: MenuBarLabels::Platforms,
            ..Config::default()
        };
        let restored: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert!(restored.menu_bar_labels == MenuBarLabels::Platforms);
    }
}
