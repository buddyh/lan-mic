//! First launch of the downloadable app offers user-level setup, without Rust.
use anyhow::{Context, Result};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy,
};
use objc2_foundation::NSString;

pub fn install_if_needed() -> Result<bool> {
    let binary = std::env::current_exe()?;
    let contents = binary
        .parent()
        .and_then(|p| p.parent())
        .context("App location")?;
    let installer = contents.join("Resources/install-mac.sh");
    // Running an ordinary developer build does not install anything.
    if !installer.is_file() {
        return Ok(false);
    }
    let installed = crate::home().join("Applications/Lan Mic.app/Contents/MacOS/LanMic");
    let agents = crate::home().join("Library/LaunchAgents");
    let identifier = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print CFBundleIdentifier"])
        .arg(contents.join("Info.plist"))
        .output()?;
    let identifier = String::from_utf8_lossy(&identifier.stdout)
        .trim()
        .to_owned();
    let has_job = !identifier.is_empty() && agents.join(format!("{identifier}.plist")).exists();
    if binary == installed && has_job {
        return Ok(false);
    }
    let mtm = MainThreadMarker::new().context("Setup must run on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.activate();
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("Set up Lan Mic on this Mac?"));
    alert.setInformativeText(&NSString::from_str("Installs Lan Mic in your Applications folder and starts it with your desktop session. LAN Mouse must already be installed and paired. Microphone access and SSH pairing remain under your control. No administrator password is needed for this step."));
    alert.addButtonWithTitle(&NSString::from_str("Install Lan Mic"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    if alert.runModal() != NSAlertFirstButtonReturn {
        return Ok(true);
    }
    let output = std::process::Command::new("/bin/bash")
        .arg(&installer)
        .arg(&binary)
        .env("LAN_MIC_SETUP_PID", std::process::id().to_string())
        .output()
        .context("Start Lan Mic installer")?;
    if !output.status.success() {
        let failure = NSAlert::new(mtm);
        failure.setMessageText(&NSString::from_str("Setup could not finish"));
        let detail = String::from_utf8_lossy(&output.stderr);
        failure.setInformativeText(&NSString::from_str(&format!(
            "Check that LAN Mouse is installed and paired, then try again.\n\n{}",
            detail.chars().take(1200).collect::<String>()
        )));
        failure.addButtonWithTitle(&NSString::from_str("OK"));
        failure.runModal();
        return Ok(true);
    }
    let opened = std::process::Command::new(&installed)
        .arg("show")
        .status()?;
    anyhow::ensure!(
        opened.success(),
        "Installed; reopen Lan Mic from Applications"
    );
    Ok(true)
}
