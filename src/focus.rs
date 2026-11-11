/// Adapter for LAN Mouse 0.11 logs. Replace with official focus events upstream.
#[derive(Default)]
pub struct Focus {
    pub remote: bool,
    pub target: u64,
    pending: Option<u64>,
}
impl Focus {
    pub fn new(target: u64) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }
    pub fn reset(&mut self) {
        self.remote = false;
        self.pending = None;
    }
    pub fn ingest(&mut self, line: &str) {
        if line.contains("input capture exited")
            || line.contains("input capture backend unavailable")
            || (line.contains("connection closed")
                && number(line, "client (", ")") == Some(self.target))
        {
            self.reset();
            return;
        }
        if [
            "using config:",
            "releasing capture:",
            "sending Leave event to client",
            "leaving client",
        ]
        .iter()
        .any(|s| line.contains(s))
        {
            self.reset();
        } else if let Some(n) = number(line, "entering client ", " ...") {
            self.pending = Some(n);
            self.remote = false;
        } else if let Some(n) = number(line, "client ", " acknowledged the connection!") {
            if self.pending == Some(n) {
                self.remote = n == self.target;
            }
        }
    }
}
fn number(line: &str, prefix: &str, suffix: &str) -> Option<u64> {
    let (_, tail) = line.split_once(prefix)?;
    let (value, _) = tail.split_once(suffix)?;
    value.parse().ok()
}

pub struct Tail {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
    inode: u64,
    pending: Vec<u8>,
}
impl Tail {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            file: None,
            inode: 0,
            pending: Vec::new(),
        }
    }
    pub fn poll(&mut self, focus: &mut Focus) -> std::io::Result<()> {
        use std::{
            io::{Read, Seek, SeekFrom},
            os::unix::fs::MetadataExt,
        };
        let meta = match self.path.metadata() {
            Ok(m) => m,
            Err(_) => {
                focus.reset();
                self.file = None;
                return Ok(());
            }
        };
        if self.file.is_none() || self.inode != meta.ino() {
            let mut file = std::fs::File::open(&self.path)?;
            file.seek(SeekFrom::End(0))?;
            self.file = Some(file);
            self.inode = meta.ino();
            self.pending.clear();
            focus.reset();
            return Ok(());
        }
        let file = self.file.as_mut().unwrap();
        if meta.len() < file.stream_position()? {
            file.rewind()?;
            self.pending.clear();
            focus.reset();
        }
        let mut data = [0; 65536];
        let n = file.read(&mut data)?;
        self.pending.extend_from_slice(&data[..n]);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            focus.ingest(&String::from_utf8_lossy(&self.pending[..end]));
            self.pending.drain(..=end);
        }
        if self.pending.len() > 65536 {
            self.pending.clear();
            focus.reset();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmed_handoff_and_late_ack() {
        let mut f = Focus::default();
        f.ingest("client 0 acknowledged the connection!");
        assert!(!f.remote);
        f.ingest("entering client 0 ...");
        assert!(!f.remote);
        f.ingest("client 0 acknowledged the connection!");
        assert!(f.remote);
        f.ingest("client 0 acknowledged the connection!");
        assert!(f.remote);
        f.ingest("releasing capture: left remote client device region");
        assert!(!f.remote);
        f.ingest("client 0 acknowledged the connection!");
        assert!(!f.remote);
    }
    #[test]
    fn other_peers_restart_and_disconnect() {
        let mut f = Focus::default();
        f.ingest("entering client 1 ...");
        f.ingest("client 1 acknowledged the connection!");
        assert!(!f.remote);
        for reset in [
            "using config:",
            "sending Leave event to client 0",
            "leaving client 0",
            "releasing capture: emergency",
        ] {
            f.ingest("entering client 0 ...");
            f.ingest("client 0 acknowledged the connection!");
            assert!(f.remote);
            f.ingest(reset);
            f.ingest("client 0 acknowledged the connection!");
            assert!(!f.remote);
        }
        f.ingest("entering client 0 ...");
        f.reset();
        f.ingest("client 0 acknowledged the connection!");
        assert!(!f.remote);
    }
    #[test]
    fn selected_peer_disconnect_invalidates_late_ack() {
        let mut f = Focus::new(3);
        f.ingest("entering client 3 ...");
        f.ingest("client 3 acknowledged the connection!");
        assert!(f.remote);
        f.ingest("client (3) @ 127.0.0.1:4242 connection closed");
        f.ingest("client 3 acknowledged the connection!");
        assert!(!f.remote);
    }
    #[test]
    fn tail_never_replays_old_sessions_and_handles_rotation() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("lan-mic-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mouse.log");
        let handoff = "entering client 0 ...\nclient 0 acknowledged the connection!\n";
        std::fs::write(&path, handoff).unwrap();
        let mut tail = Tail::new(path.clone());
        let mut f = Focus::default();
        tail.poll(&mut f).unwrap();
        assert!(!f.remote);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(handoff.as_bytes()).unwrap();
        tail.poll(&mut f).unwrap();
        assert!(f.remote);
        std::fs::rename(&path, dir.join("old.log")).unwrap();
        std::fs::write(&path, handoff).unwrap();
        tail.poll(&mut f).unwrap();
        assert!(!f.remote);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"client 0 acknowledged the connection!\n")
            .unwrap();
        tail.poll(&mut f).unwrap();
        assert!(!f.remote);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
