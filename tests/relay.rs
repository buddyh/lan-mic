use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[test]
fn relay_flushes_binary_frames_without_waiting_for_a_newline() {
    let dir = std::env::temp_dir().join(format!("lan-mic-relay-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join("audio.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lan-mic"))
        .arg("relay")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    let payload = b"S\0\0\0\x02{}";
    let mut output = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut got = [0; 7];
        let result = output.read_exact(&mut got).map(|_| got);
        let _ = tx.send(result);
    });
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    child.stdin.as_mut().unwrap().write_all(payload).unwrap();
    let mut incoming = [0; 7];
    peer.read_exact(&mut incoming).unwrap();
    assert_eq!(&incoming, payload);
    peer.write_all(payload).unwrap();
    let received = rx.recv_timeout(Duration::from_secs(1));
    let _ = child.kill();
    let _ = child.wait();
    reader.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
    assert_eq!(
        &received
            .expect("Relay buffered a frame instead of flushing")
            .unwrap(),
        payload
    );
}
