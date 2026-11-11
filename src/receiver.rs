use crate::{
    atomic_json,
    config::{self, Config},
    focus::{Focus, Tail},
    now,
    route::{self, Pactl, Pulse, Route, SINK, SOURCE},
    wire,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs::OpenOptions,
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::{Child, Command},
};
fn publish(path: &Path, mut value: Value) -> Result<()> {
    value["updated"] = json!(now());
    atomic_json(path, &value)
}
async fn send(writer: &mut (impl AsyncWrite + Unpin), kind: u8, payload: &[u8]) -> Result<()> {
    tokio::time::timeout(
        Duration::from_millis(200),
        writer.write_all(&wire::encode(kind, payload)?),
    )
    .await
    .context("Peer output stalled")??;
    Ok(())
}
fn daemon_alive() -> bool {
    std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|p| {
        std::os::unix::net::UnixStream::connect(
            std::path::PathBuf::from(p).join("lan-mouse-socket.sock"),
        )
        .is_ok()
    })
}
async fn microphone(config: &Config) -> Result<(Child, String)> {
    let mut pulse = Pactl;
    let source = match &config.input_source {
        Some(s) => s.clone(),
        None => pulse.call(&["get-default-source"]).await?,
    };
    ensure!(
        safe_input(&source),
        "Choose a local microphone; monitor/received sources cannot be forwarded"
    );
    let list: Vec<Value> =
        serde_json::from_str(&pulse.call(&["-f", "json", "list", "sources"]).await?)?;
    ensure!(
        list.iter().any(|s| s["name"] == source),
        "Selected microphone is unavailable"
    );
    let child = Command::new("parec")
        .args([
            "--record",
            &format!("--device={source}"),
            "--rate=48000",
            "--channels=1",
            "--format=s16le",
            "--latency-msec=40",
            "--process-time-msec=10",
            "--client-name=LanMicCapture",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    Ok((child, source))
}
fn safe_input(source: &str) -> bool {
    source != SOURCE && source != format!("{SINK}.monitor") && !source.ends_with(".monitor")
}
async fn session<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut reader: R,
    mut writer: W,
    config: &Config,
    route: &mut Route<Pactl>,
    path: &Path,
) -> Result<()> {
    let mut player = Command::new("pacat")
        .args([
            "--playback",
            &format!("--device={SINK}"),
            "--rate=48000",
            "--channels=1",
            "--format=s16le",
            "--latency-msec=40",
            "--process-time-msec=10",
            "--client-name=LanMic",
        ])
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut capture: Option<Child> = None;
    let mut capture_source = String::new();
    let mut source_error = None;
    let mut focus = Focus::new(config.target_client);
    let mut tail = Tail::new(config.lan_mouse_log.clone());
    let mut allowed = false;
    let mut peer_ready = true;
    let mut peer_name = config
        .peer_name
        .clone()
        .unwrap_or_else(|| "Other computer".into());
    let mut decoder = wire::Decoder::default();
    let mut input = [0u8; 65536];
    let mut audio = [0u8; 3840];
    let mut pcm = Vec::new();
    let mut last_message = Instant::now();
    let mut last_audio = Instant::now();
    let mut last_heartbeat = Instant::now() - Duration::from_secs(1);
    let mut retry = last_heartbeat;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut previous = Vec::new();
    let mut daemon = daemon_alive();
    let mut last_daemon = Instant::now();
    let mut last_route_refresh = Instant::now();
    loop {
        tokio::select! {
            result=reader.read(&mut input)=>{
                let n=result?;if n==0{bail!("Peer disconnected");}decoder.feed(&input[..n])?;
                while let Some((kind,payload))=decoder.pop()?{
                    last_message=Instant::now();
                    if kind==b'S'{
                        let state:Value=serde_json::from_slice(&payload)?;ensure!(state["protocol"]==1,"Unsupported helper protocol");
                        if config.peer_name.is_none(){if let Some(name)=state["computer_name"].as_str(){peer_name=config::clean_name(name);}}
                        peer_ready=state["receive_ready"].as_bool().unwrap_or(true);
                        allowed=state["capturing"]==true&&daemon;
                        if !allowed&&route.active{route.deactivate().await?;}
                    }else if allowed&&!focus.remote&&!payload.is_empty(){
                        last_audio=Instant::now();if !route.active{route.activate().await?;}
                        tokio::time::timeout(Duration::from_millis(100),player.stdin.as_mut().unwrap().write_all(&payload)).await.context("Audio player stalled")??;
                    }
                }
            },
            result=async{capture.as_mut().unwrap().stdout.as_mut().unwrap().read(&mut audio).await},if capture.is_some()=>{
                let n=result?;if n==0{capture=None;source_error=Some("Microphone capture stopped".to_owned());retry=Instant::now();}
                else{
                    pcm.extend_from_slice(&audio[..n]);let paired=pcm.len()/2*2;
                    if paired>0{send(&mut writer,b'A',&pcm[..paired]).await?;pcm.drain(..paired);}
                }
            },
            _=tick.tick()=>{
                if let Err(error)=tail.poll(&mut focus){eprintln!("LAN Mouse log: {error}");focus.reset();}
                if last_daemon.elapsed()>Duration::from_secs(1){daemon=daemon_alive();last_daemon=Instant::now();}
                if !daemon{focus.reset();allowed=false;if route.active{route.deactivate().await?;}}
                if last_message.elapsed()>Duration::from_secs(3){bail!("Peer heartbeat timed out");}
                if route.active&&last_audio.elapsed()>Duration::from_secs(2){bail!("Remote microphone audio timed out");}
                if player.try_wait()?.is_some(){bail!("Audio player exited");}
                if route.active&&last_route_refresh.elapsed()>Duration::from_millis(250){
                    route.refresh().await?;last_route_refresh=Instant::now();
                }
            }
        }
        if focus.remote && route.active {
            route.deactivate().await?;
        }
        let desired = focus.remote && peer_ready;
        if !desired {
            capture = None;
            pcm.clear();
            source_error = None;
        }
        if desired && capture.is_none() && retry.elapsed() > Duration::from_secs(2) {
            retry = Instant::now();
            match microphone(config).await {
                Ok((child, source)) => {
                    capture = Some(child);
                    capture_source = source;
                    source_error = None;
                }
                Err(error) => {
                    source_error = Some(error.to_string());
                }
            }
        }
        let reason = if let Some(error) = &source_error {
            error.as_str()
        } else if capture.is_some() {
            "Sending the local microphone"
        } else if route.active {
            "Receiving the other computer’s microphone"
        } else if !peer_ready {
            "The other computer needs a virtual microphone before it can receive audio"
        } else {
            "Microphone stays local until LAN Mouse hands off control"
        };
        let state = json!({"protocol":1,"implementation":"rust","platform":"linux","computer_name":config.computer_name,"peer_name":peer_name,"capturing":capture.is_some(),"focused":focus.remote,"receive_ready":daemon,"receiving":route.active,"forwarding":capture.is_some()||route.active,"input":capture_source,"reason":reason});
        let encoded = serde_json::to_vec(&state)?;
        if encoded != previous || last_heartbeat.elapsed() > Duration::from_millis(500) {
            // Announce capture before the select loop can send its first PCM.
            send(&mut writer, b'S', &encoded).await?;
            publish(path, state)?;
            previous = encoded;
            last_heartbeat = Instant::now();
        }
    }
}
async fn connected(
    host: &str,
    remote: &str,
    config: &Config,
    route: &mut Route<Pactl>,
    path: &Path,
) -> Result<()> {
    ensure!(
        !host.starts_with('-') && !host.is_empty(),
        "SSH host cannot be empty or start with '-'"
    );
    let mut ssh = Command::new("ssh")
        .args([
            "-T",
            "-oBatchMode=yes",
            "-oStrictHostKeyChecking=yes",
            "-oConnectTimeout=5",
            "-oServerAliveInterval=3",
            "-oServerAliveCountMax=2",
            "-oCompression=no",
            host,
            remote,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let output = ssh.stdout.take().unwrap();
    let input = ssh.stdin.take().unwrap();
    eprintln!("Connected transport to {host}");
    session(output, input, config, route, path).await
}
pub async fn run(host: Option<&str>, remote: &str, status: bool) -> Result<()> {
    let dir = config::state_dir();
    let path = dir.join("status.json");
    if status {
        println!(
            "{}",
            std::fs::read_to_string(path).unwrap_or_else(|_| "{\"reason\":\"Not started\"}".into())
        );
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("receiver.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("Lan Mic is already running; use receiver --status");
    }
    let config = Config::load()?;
    route::ensure_source().await?;
    let mut route = Route::new(Pactl, Some(dir.join("route.json")));
    route.recover().await?;
    let socket_path = dir.join("audio.sock");
    let listener = if host.is_none() {
        if std::os::unix::net::UnixStream::connect(&socket_path).is_ok() {
            bail!("Another Lan Mic instance owns the socket");
        }
        let _ = std::fs::remove_file(&socket_path);
        let l = tokio::net::UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        Some(l)
    } else {
        None
    };
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        publish(
            &path,
            json!({"forwarding":false,"computer_name":config.computer_name,"reason":"Waiting for peer connection"}),
        )?;
        let session = async {
            if let Some(host) = host {
                connected(host, remote, &config, &mut route, &path).await
            } else {
                let (stream, _) = listener.as_ref().unwrap().accept().await?;
                let (r, w) = stream.into_split();
                session(r, w, &config, &mut route, &path).await
            }
        };
        let stopping = tokio::select! {result=session=>{if let Err(error)=result{eprintln!("Peer connection unavailable: {error:#}");}false},_=terminate.recv()=>true,_=tokio::signal::ctrl_c()=>true};
        let restored = route.deactivate().await;
        publish(
            &path,
            json!({"forwarding":false,"reason":if stopping{"Stopped"}else{"Disconnected"}}),
        )?;
        restored?;
        if stopping {
            break;
        }
        tokio::select! {_=tokio::time::sleep(Duration::from_secs(2))=>{},_=terminate.recv()=>break,_=tokio::signal::ctrl_c()=>break}
    }
    if listener.is_some() {
        let _ = std::fs::remove_file(socket_path);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_audio_feedback_sources() {
        for source in [
            SOURCE,
            "alsa_output.speaker.monitor",
            "lan_mic_bridge.monitor",
        ] {
            assert!(!safe_input(source));
        }
        assert!(safe_input("alsa_input.usb-Microphone"));
    }
}
