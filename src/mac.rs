use crate::{
    atomic_json,
    focus::{Focus, Tail},
    home,
    mac_ui::{Action, Ui},
    now,
    pcm::Pcm,
    wire,
};
use anyhow::{bail, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use serde_json::json;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
        net::{UnixListener, UnixStream},
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder,
};

struct Capture {
    _stream: cpal::Stream,
    queue: Receiver<(Instant, Vec<f32>)>,
    failed: Arc<AtomicBool>,
    pcm: Pcm,
    last: Instant,
    device: String,
}
fn input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    tx: mpsc::SyncSender<(Instant, Vec<f32>)>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let error = failed.clone();
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let samples = data
                .iter()
                .map(|s| <f32 as cpal::Sample>::from_sample(*s))
                .collect();
            if tx.try_send((Instant::now(), samples)).is_err() {
                failed.store(true, Ordering::Relaxed);
            }
        },
        move |e| {
            eprintln!("Microphone: {e}");
            error.store(true, Ordering::Relaxed);
        },
        None,
    )?)
}
impl Capture {
    fn start(config: &crate::config::Config) -> Result<Self> {
        let host = cpal::default_host();
        let device = if let Some(selected) = &config.input_source {
            host.input_devices()?
                .find(|d| {
                    d.id().is_ok_and(|id| id.to_string() == *selected)
                        || d.description().is_ok_and(|v| v.name() == selected)
                })
                .context("Selected microphone is unavailable")?
        } else {
            host.default_input_device()
                .context("No default Mac microphone")?
        };
        anyhow::ensure!(device.description()?.name()!="BlackHole 2ch","Choose a local microphone; forwarding the received virtual microphone would create a feedback loop");
        let id = device.id()?.to_string();
        let format = device.default_input_config()?;
        let pcm = Pcm::new(format.sample_rate(), format.channels())?;
        let (tx, queue) = mpsc::sync_channel(16);
        let mut stream_config: cpal::StreamConfig = format.into();
        if let cpal::SupportedBufferSize::Range { min, max } = format.buffer_size() {
            let desired = 512u32.clamp(*min, *max);
            stream_config.buffer_size = cpal::BufferSize::Fixed(desired);
        }
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match format.sample_format() {
            SampleFormat::F32 => input_stream::<f32>(&device, stream_config, tx, failed.clone())?,
            SampleFormat::I16 => input_stream::<i16>(&device, stream_config, tx, failed.clone())?,
            SampleFormat::I32 => input_stream::<i32>(&device, stream_config, tx, failed.clone())?,
            SampleFormat::U16 => input_stream::<u16>(&device, stream_config, tx, failed.clone())?,
            SampleFormat::F64 => input_stream::<f64>(&device, stream_config, tx, failed.clone())?,
            other => bail!("Unsupported microphone format: {other}"),
        };
        stream.play()?;
        Ok(Self {
            _stream: stream,
            queue,
            failed,
            pcm,
            last: Instant::now(),
            device: id,
        })
    }
    fn pump(&mut self, peer: &mut UnixStream) -> Result<()> {
        if self.failed.load(Ordering::Relaxed) || self.last.elapsed() > Duration::from_secs(3) {
            bail!("Microphone input stalled or changed");
        }
        while let Ok((at, samples)) = self.queue.try_recv() {
            if at.elapsed() > Duration::from_millis(250) {
                bail!("Microphone queue stalled");
            }
            self.last = at;
            let pcm = self.pcm.convert(&samples)?;
            for bytes in pcm.chunks(3840) {
                if !bytes.is_empty() {
                    peer.write_all(&wire::encode(b'A', bytes)?)?;
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn permission() -> bool {
    unsafe {
        AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio.unwrap())
            == AVAuthorizationStatus::Authorized
    }
}
fn request_permission() {
    unsafe {
        if AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeAudio.unwrap())
            == AVAuthorizationStatus::NotDetermined
        {
            let callback = block2::RcBlock::new(|_: objc2::runtime::Bool| {});
            AVCaptureDevice::requestAccessForMediaType_completionHandler(
                AVMediaTypeAudio.unwrap(),
                &callback,
            );
        }
    }
}
fn send_status(peer: &mut Option<UnixStream>, bytes: &[u8]) {
    if peer
        .as_mut()
        .is_some_and(|p| p.write_all(&wire::encode(b'S', bytes).unwrap()).is_err())
    {
        *peer = None;
    }
}
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}
pub fn run(background: bool) -> Result<()> {
    let mut config = crate::config::Config::load()?;
    let mtm = MainThreadMarker::new().context("Mac app must run on the main thread")?;
    let support = home().join("Library/Application Support/Lan Mic");
    std::fs::create_dir_all(&support)?;
    std::fs::set_permissions(&support, std::fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(support.join("agent.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return crate::mac_ui::request("show");
    }
    let socket_path = support.join("audio.sock");
    if UnixStream::connect(&socket_path).is_ok() {
        bail!("Another Lan Mic instance owns the socket");
    }
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let ui = Ui::new(app.clone(), mtm, config.menu_bar_labels)?;
    let prompt_path = support.join("support-prompt.json");
    let mut support_prompt: crate::support::SupportPrompt = std::fs::read(&prompt_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut last_prompt_tick = Instant::now();
    let ui_path = support.join("ui.sock");
    let _ = std::fs::remove_file(&ui_path);
    let ui_listener = UnixListener::bind(&ui_path)?;
    std::fs::set_permissions(&ui_path, std::fs::Permissions::from_mode(0o600))?;
    ui_listener.set_nonblocking(true)?;
    let menu = Menu::new();
    let state_item = MenuItem::new("Starting Lan Mic…", false, None);
    let open_window = MenuItem::new("Settings…", true, None);
    let mute = MenuItem::new("Mute forwarding", true, None);
    let quit = MenuItem::new("Quit Lan Mic", true, None);
    menu.append_items(&[
        &state_item,
        &open_window,
        &mute,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;
    app.finishLaunching();
    if !background {
        ui.show();
    }
    let tray = TrayIconBuilder::new()
        .with_title(format!("🎙 {}", config.computer_name))
        .with_menu(Box::new(menu))
        .build()?;
    request_permission();
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    let (update_tx, update_rx) = std::sync::mpsc::sync_channel(1);
    let (command_tx, command_rx) = std::sync::mpsc::channel();
    let engine_config = config.clone();
    let engine_support = support.clone();
    let engine = std::thread::spawn(move || {
        engine(
            engine_config,
            listener,
            engine_support,
            socket_path,
            update_tx,
            command_rx,
        )
    });
    while !STOP.load(Ordering::Relaxed) && !engine.is_finished() {
        let mut actions = ui.poll();
        while let Ok((mut connection, _)) = ui_listener.accept() {
            connection.set_read_timeout(Some(Duration::from_millis(100)))?;
            let mut bytes = [0; 64];
            let count = connection.read(&mut bytes).unwrap_or(0);
            match std::str::from_utf8(&bytes[..count]).unwrap_or("").trim() {
                "show" => ui.show(),
                "about" => ui.show_about(),
                "mute" => actions.push(Action::Mute),
                "close" => ui.window.performClose(None),
                "preview" => {
                    let _ = ui.preview();
                }
                _ => {}
            }
            let _ = connection.write_all(&serde_json::to_vec(&ui.inspect())?);
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == quit.id() {
                actions.push(Action::Quit);
            }
            if event.id == open_window.id() {
                actions.push(Action::Show);
            }
            if event.id == mute.id() {
                actions.push(Action::Mute);
            }
        }
        for action in actions {
            match action {
                Action::Quit => {
                    STOP.store(true, Ordering::Relaxed);
                }
                Action::Show => ui.show(),
                Action::About => ui.show_about(),
                Action::Star => {
                    support_prompt.mark_shown();
                    let _ = atomic_json(&prompt_path, &support_prompt);
                    ui.dismiss_support();
                    let _ = std::process::Command::new("open")
                        .arg(env!("CARGO_PKG_REPOSITORY"))
                        .spawn();
                }
                Action::DismissSupport => ui.dismiss_support(),
                Action::License => {
                    if let Some(contents) =
                        std::env::current_exe()?.parent().and_then(|p| p.parent())
                    {
                        let _ = std::process::Command::new("open")
                            .arg(contents.join("Resources/LICENSE"))
                            .spawn();
                    }
                }
                Action::Mute => {
                    let _ = command_tx.send(());
                }
                Action::MenuBarLabels => {
                    config.menu_bar_labels = ui.menu_bar_labels();
                    let mut saved = crate::config::Config::load()?;
                    saved.menu_bar_labels = config.menu_bar_labels;
                    saved.save()?;
                }
                Action::Permission => {
                    request_permission();
                    let _=std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone").spawn();
                }
                Action::Diagnostics => {
                    let _ = std::process::Command::new("open")
                        .arg(support.join("status.json"))
                        .spawn();
                }
            }
        }
        while let Ok(state) = update_rx.try_recv() {
            let remote = state["peer_name"].as_str().unwrap_or("Other computer");
            let muted = state["muted"] == true;
            let capturing = state["capturing"] == true;
            let receiving = state["receiving"] == true;
            if support_prompt.update(
                capturing || receiving,
                last_prompt_tick.elapsed().as_secs_f64(),
            ) {
                let _ = atomic_json(&prompt_path, &support_prompt);
            }
            last_prompt_tick = Instant::now();
            let reason = state["reason"].as_str().unwrap_or("Checking status");
            ui.update(&state);
            state_item.set_text(reason);
            mute.set_text(if muted {
                "Unmute forwarding"
            } else {
                "Mute forwarding"
            });
            let mode = if muted {
                crate::config::DisplayState::Muted
            } else if capturing {
                crate::config::DisplayState::Sending
            } else if receiving {
                crate::config::DisplayState::Receiving
            } else {
                crate::config::DisplayState::Idle
            };
            let title = crate::config::tray_title(
                config.menu_bar_labels,
                &config.computer_name,
                remote,
                "macos",
                state["peer_platform"].as_str().unwrap_or("linux"),
                mode,
            );
            tray.set_title(Some(&title));
        }
        if support_prompt.due(ui.support_prompt_ready(), true) {
            // Persist before showing: closing, dismissing, or restarting never nags.
            support_prompt.mark_shown();
            if atomic_json(&prompt_path, &support_prompt).is_ok() {
                ui.show_support();
            }
        }
    }
    STOP.store(true, Ordering::Relaxed);
    let result = engine
        .join()
        .map_err(|_| anyhow::anyhow!("Audio engine thread failed"))?;
    let _ = std::fs::remove_file(ui_path);
    result
}
fn engine(
    config: crate::config::Config,
    listener: UnixListener,
    support: std::path::PathBuf,
    socket_path: std::path::PathBuf,
    updates: std::sync::mpsc::SyncSender<serde_json::Value>,
    commands: std::sync::mpsc::Receiver<()>,
) -> Result<()> {
    let mut tail = Tail::new(config.lan_mouse_log.clone());
    let mut focus = Focus::new(config.target_client);
    let mut incoming = crate::mac_audio::Incoming::new()?;
    let mut decoder = wire::Decoder::default();
    let mut receive_allowed = false;
    let mut peer_name = config
        .peer_name
        .clone()
        .unwrap_or_else(|| "Other computer".into());
    let mut peer_ready = true;
    let mut last_peer = Instant::now();
    let mut last_remote_audio = Instant::now();
    let mut receive_ready = crate::mac_audio::Incoming::available();
    let mut input_name = "System default".to_owned();
    let mut capture_error = None;
    let mut peer: Option<UnixStream> = None;
    let mut capture: Option<Capture> = None;
    let mut muted = false;
    let mut daemon = false;
    let mut last_check = Instant::now() - Duration::from_secs(2);
    let mut heartbeat = last_check;
    let mut retry = last_check;
    let mut previous = Vec::new();
    loop {
        if STOP.load(Ordering::Relaxed) {
            capture.take();
            incoming.stop()?;
            let state = json!({"protocol":1,"capturing":false,"focused":false,"reason":"Stopped","updated":now()});
            send_status(&mut peer, &serde_json::to_vec(&state)?);
            atomic_json(&support.join("status.json"), &state)?;
            let _ = std::fs::remove_file(&socket_path);
            return Ok(());
        }
        while commands.try_recv().is_ok() {
            muted = !muted;
        }
        if let Err(error) = tail.poll(&mut focus) {
            eprintln!("LAN Mouse log: {error}");
            focus.reset();
        }
        while let Ok((socket, _)) = listener.accept() {
            if peer.is_none() {
                socket.set_write_timeout(Some(Duration::from_millis(100)))?;
                peer = Some(socket);
                decoder = wire::Decoder::default();
                last_peer = Instant::now();
                receive_allowed = false;
                heartbeat = Instant::now() - Duration::from_secs(1);
            }
        }
        if peer.is_some() {
            let result = (|| -> Result<()> {
                let mut bytes = [0u8; 65536];
                for _ in 0..4 {
                    let count = unsafe {
                        libc::recv(
                            peer.as_ref().unwrap().as_raw_fd(),
                            bytes.as_mut_ptr().cast(),
                            bytes.len(),
                            libc::MSG_DONTWAIT,
                        )
                    };
                    if count == 0 {
                        bail!("Peer disconnected");
                    }
                    if count < 0 {
                        let e = std::io::Error::last_os_error();
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            break;
                        }
                        return Err(e.into());
                    }
                    decoder.feed(&bytes[..count as usize])?;
                    while let Some((kind, payload)) = decoder.pop()? {
                        last_peer = Instant::now();
                        if kind == b'S' {
                            let state: serde_json::Value = serde_json::from_slice(&payload)?;
                            anyhow::ensure!(state["protocol"] == 1, "Unsupported peer protocol");
                            if config.peer_name.is_none() {
                                if let Some(name) = state["computer_name"].as_str() {
                                    peer_name = crate::config::clean_name(name);
                                }
                            }
                            peer_ready = state["receive_ready"].as_bool().unwrap_or(true);
                            receive_allowed =
                                state["capturing"] == true && !muted && !focus.remote && daemon;
                            if !receive_allowed {
                                incoming.stop()?;
                            }
                        } else if receive_allowed
                            && !muted
                            && !focus.remote
                            && daemon
                            && !payload.is_empty()
                        {
                            incoming.push(&payload)?;
                            last_remote_audio = Instant::now();
                        }
                    }
                }
                anyhow::ensure!(
                    last_peer.elapsed() < Duration::from_secs(3),
                    "Peer heartbeat timed out"
                );
                if incoming.active {
                    anyhow::ensure!(
                        last_remote_audio.elapsed() < Duration::from_secs(2),
                        "Remote microphone stalled"
                    );
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Incoming audio stopped: {error:#}");
                peer = None;
                receive_allowed = false;
                incoming.stop()?;
            }
        }
        if muted || focus.remote || peer.is_none() || !daemon {
            incoming.stop()?;
        }
        if last_check.elapsed() > Duration::from_secs(1) {
            last_check = Instant::now();
            receive_ready = crate::mac_audio::Incoming::available();
            if let Some(device) = cpal::default_host().default_input_device() {
                if let Ok(description) = device.description() {
                    input_name = description.name().to_owned();
                }
            }
            daemon =
                UnixStream::connect(home().join("Library/Caches/lan-mouse-socket.sock")).is_ok();
            if !daemon {
                focus.reset();
            }
            if let Some(c) = &capture {
                let host = cpal::default_host();
                let still_selected = if config.input_source.is_some() {
                    host.input_devices().is_ok_and(|mut devices| {
                        devices.any(|d| d.id().is_ok_and(|id| id.to_string() == c.device))
                    })
                } else {
                    host.default_input_device()
                        .and_then(|d| d.id().ok())
                        .is_some_and(|id| id.to_string() == c.device)
                };
                if !still_selected {
                    capture = None;
                }
            }
        }
        let authorized = permission();
        let desired =
            focus.remote && !muted && peer.is_some() && peer_ready && daemon && authorized;
        if !desired {
            capture = None;
            capture_error = None;
        }
        if desired && capture.is_none() && retry.elapsed() > Duration::from_secs(1) {
            retry = Instant::now();
            match Capture::start(&config) {
                Ok(c) => {
                    capture = Some(c);
                    capture_error = None;
                }
                Err(e) => {
                    eprintln!("Capture: {e:#}");
                    capture_error = Some(format!("Microphone error: {e}"));
                }
            }
        }
        let reason = if muted {
            "Forwarding muted"
        } else if !authorized {
            "Allow microphone access in Privacy & Security"
        } else if !daemon {
            "LAN Mouse is not running"
        } else if peer.is_none() {
            "Other computer disconnected"
        } else if incoming.active {
            "Receiving the other computer’s microphone"
        } else if !peer_ready && focus.remote {
            "The other computer is not ready to receive a microphone"
        } else if !focus.remote {
            "Microphone stays local until LAN Mouse hands off control"
        } else if capture.is_some() {
            "Microphone follows keyboard and mouse to the other computer"
        } else {
            "Waiting for usable microphone"
        };
        let status = json!({"protocol":1,"implementation":"rust","platform":"macos","peer_platform":"linux","focused":focus.remote,"capturing":capture.is_some(),"muted":muted,"receiving":incoming.active,"receive_ready":receive_ready&&daemon,"virtual_microphone_available":receive_ready,"computer_name":config.computer_name,"peer_name":peer_name,"input":input_name,"daemon":daemon,"connected":peer.is_some(),"authorized":authorized,"reason":capture_error.as_deref().unwrap_or(reason)});
        let encoded = serde_json::to_vec(&status)?;
        // Announce permission/focus before sending the first sample of a session.
        if encoded != previous || heartbeat.elapsed() > Duration::from_millis(500) {
            let mut file_status = status;
            file_status["updated"] = json!(now());
            atomic_json(&support.join("status.json"), &file_status)?;
            let _ = updates.try_send(file_status.clone());
            send_status(&mut peer, &encoded);
            previous = encoded;
            heartbeat = Instant::now();
        }
        if let (Some(c), Some(p)) = (&mut capture, &mut peer) {
            if let Err(error) = c.pump(p) {
                eprintln!("Audio stopped: {error:#}");
                capture = None;
                peer = None;
                retry = Instant::now();
            }
        }
        if peer.is_none() {
            capture = None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
