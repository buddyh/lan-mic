use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

pub const SOURCE: &str = "lan_mic";
pub const SINK: &str = "lan_mic_bridge";
pub trait Pulse {
    async fn call(&mut self, args: &[&str]) -> Result<String>;
}
pub struct Pactl;
impl Pulse for Pactl {
    async fn call(&mut self, args: &[&str]) -> Result<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(4),
            tokio::process::Command::new("pactl")
                .args(args)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        if !output.status.success() {
            bail!(
                "pactl {:?}: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    previous: Option<String>,
    moved: HashMap<String, String>,
}
pub struct Route<P: Pulse> {
    pub pulse: P,
    pub active: bool,
    saved: Saved,
    path: Option<PathBuf>,
    observed: HashSet<String>,
}
impl<P: Pulse> Route<P> {
    pub fn new(pulse: P, path: Option<PathBuf>) -> Self {
        Self {
            pulse,
            path,
            active: false,
            saved: Saved::default(),
            observed: HashSet::new(),
        }
    }
    fn save(&self) -> Result<()> {
        if let Some(p) = &self.path {
            crate::atomic_json(p, &self.saved)?;
        }
        Ok(())
    }
    async fn sources(&mut self) -> Result<HashMap<String, String>> {
        let rows: Vec<Value> =
            serde_json::from_str(&self.pulse.call(&["-f", "json", "list", "sources"]).await?)?;
        Ok(rows
            .iter()
            .filter_map(|x| {
                Some((
                    x["index"].as_u64()?.to_string(),
                    x["name"].as_str()?.to_owned(),
                ))
            })
            .collect())
    }
    async fn streams(&mut self) -> Result<Vec<(String, String)>> {
        let rows: Vec<Value> = serde_json::from_str(
            &self
                .pulse
                .call(&["-f", "json", "list", "source-outputs"])
                .await?,
        )?;
        Ok(rows
            .iter()
            .filter_map(|x| {
                Some((
                    x["index"].as_u64()?.to_string(),
                    x["source"].as_u64()?.to_string(),
                ))
            })
            .collect())
    }
    pub async fn activate(&mut self) -> Result<()> {
        if !self.active {
            let previous = self.pulse.call(&["get-default-source"]).await?;
            self.saved.previous = (previous != SOURCE).then_some(previous);
            self.save()?;
            self.pulse.call(&["set-source-mute", SOURCE, "1"]).await?;
            self.pulse.call(&["set-default-source", SOURCE]).await?;
            self.active = true;
            eprintln!("Microphone follows LAN Mouse to Linux");
        }
        self.refresh().await?;
        self.pulse.call(&["set-source-mute", SOURCE, "0"]).await?;
        Ok(())
    }
    /// Apps can cache the former default and open a stream after handoff.
    /// Move each newly observed stream once, then respect manual stream changes.
    pub async fn refresh(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let follow = self.pulse.call(&["get-default-source"]).await? == SOURCE;
        let names = self.sources().await?;
        let streams = self.streams().await?;
        self.observed
            .retain(|id| streams.iter().any(|(index, _)| index == id));
        for (index, source) in streams {
            if !self.observed.insert(index.clone()) || !follow {
                continue;
            }
            if let Some(current) = names.get(&source) {
                if Some(current) == self.saved.previous.as_ref() {
                    self.saved.moved.insert(index.clone(), current.clone());
                    self.save()?;
                    self.pulse
                        .call(&["move-source-output", &index, SOURCE])
                        .await?;
                }
            }
        }
        Ok(())
    }
    pub async fn deactivate(&mut self) -> Result<()> {
        let _ = self.pulse.call(&["set-source-mute", SOURCE, "1"]).await;
        let names = self.sources().await?;
        let available: HashSet<_> = names.values().cloned().collect();
        let fallback = self
            .saved
            .previous
            .clone()
            .filter(|x| available.contains(x));
        for (index, source) in self.streams().await? {
            let original = self
                .saved
                .moved
                .get(&index)
                .filter(|x| available.contains(*x));
            let target = original.or(fallback.as_ref());
            if names.get(&source).is_some_and(|x| x == SOURCE) {
                if let Some(target) = target {
                    let _ = self
                        .pulse
                        .call(&["move-source-output", &index, target])
                        .await;
                }
            }
        }
        if let Some(previous) = fallback {
            if self.pulse.call(&["get-default-source"]).await? == SOURCE {
                self.pulse.call(&["set-default-source", &previous]).await?;
            }
        }
        if self.active {
            eprintln!("Microphone returned to Mac; Linux source muted");
        }
        self.active = false;
        self.saved = Saved::default();
        self.observed.clear();
        self.save()?;
        Ok(())
    }
    pub async fn recover(&mut self) -> Result<()> {
        if let Some(path) = &self.path {
            if path.exists() {
                self.saved = serde_json::from_slice(&std::fs::read(path)?)?;
            }
        }
        self.deactivate().await
    }
}
pub async fn ensure_source() -> Result<()> {
    let mut pulse = Pactl;
    let source = pulse.call(&["get-default-source"]).await?;
    let sink = pulse.call(&["get-default-sink"]).await?;
    let sinks: Vec<Value> =
        serde_json::from_str(&pulse.call(&["-f", "json", "list", "sinks"]).await?)?;
    if !sinks.iter().any(|x| x["name"] == SINK) {
        pulse
            .call(&[
                "load-module",
                "module-null-sink",
                &format!("sink_name={SINK}"),
                "rate=48000",
                "channels=1",
                "channel_map=mono",
                "sink_properties=device.description=LanMicTransport node.virtual=true",
            ])
            .await?;
    }
    let sources: Vec<Value> =
        serde_json::from_str(&pulse.call(&["-f", "json", "list", "sources"]).await?)?;
    if !sources.iter().any(|x| x["name"] == SOURCE) {
        pulse
            .call(&[
                "load-module",
                "module-remap-source",
                &format!("master={SINK}.monitor"),
                &format!("source_name={SOURCE}"),
                "channels=1",
                "channel_map=mono",
                "source_properties=device.description=LAN_Microphone node.virtual=true",
            ])
            .await?;
    }
    pulse.call(&["set-source-mute", SOURCE, "1"]).await?;
    if sink != SINK && pulse.call(&["get-default-sink"]).await? == SINK {
        pulse.call(&["set-default-sink", &sink]).await?;
    }
    if source != SOURCE && pulse.call(&["get-default-source"]).await? == SOURCE {
        pulse.call(&["set-default-source", &source]).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Fake {
        default: String,
        streams: HashMap<String, String>,
        muted: bool,
    }
    impl Default for Fake {
        fn default() -> Self {
            Self {
                default: "local".into(),
                streams: HashMap::from([("10".into(), "1".into()), ("11".into(), "3".into())]),
                muted: true,
            }
        }
    }
    impl Pulse for Fake {
        async fn call(&mut self, args: &[&str]) -> Result<String> {
            let names = HashMap::from([("1", "local"), ("2", SOURCE), ("3", "headset")]);
            Ok(match args {
                ["get-default-source"]=>self.default.clone(),
                ["-f","json","list","sources"]=>serde_json::to_string(&names.iter().map(|(i,n)|json!({"index":i.parse::<u64>().unwrap(),"name":n})).collect::<Vec<_>>())?,
                ["-f","json","list","source-outputs"]=>serde_json::to_string(&self.streams.iter().map(|(i,s)|json!({"index":i.parse::<u64>().unwrap(),"source":s.parse::<u64>().unwrap()})).collect::<Vec<_>>())?,
                ["set-default-source",s]=>{self.default=s.to_string();String::new()},
                ["set-source-mute",_,s]=>{self.muted=*s=="1";String::new()},
                ["move-source-output",i,s]=>{self.streams.insert(i.to_string(),names.iter().find(|(_,n)|**n==*s).unwrap().0.to_string());String::new()},
                _=>panic!("unexpected {args:?}")
            })
        }
    }
    #[tokio::test]
    async fn restores_routes_and_new_streams() {
        let mut r = Route::new(Fake::default(), None);
        r.activate().await.unwrap();
        assert_eq!(r.pulse.default, SOURCE);
        assert_eq!(r.pulse.streams["10"], "2");
        assert_eq!(r.pulse.streams["11"], "3");
        assert!(!r.pulse.muted);
        r.pulse.streams.insert("12".into(), "2".into());
        r.deactivate().await.unwrap();
        assert_eq!(r.pulse.default, "local");
        assert_eq!(r.pulse.streams["10"], "1");
        assert_eq!(r.pulse.streams["12"], "1");
        assert!(r.pulse.muted);
    }
    #[tokio::test]
    async fn preserves_manual_changes() {
        let mut r = Route::new(Fake::default(), None);
        r.activate().await.unwrap();
        r.pulse.default = "headset".into();
        r.pulse.streams.insert("10".into(), "3".into());
        r.deactivate().await.unwrap();
        assert_eq!(r.pulse.default, "headset");
        assert_eq!(r.pulse.streams["10"], "3");
    }
    #[tokio::test]
    async fn late_recording_stream_follows_once_without_overriding_manual_changes() {
        let mut r = Route::new(Fake::default(), None);
        r.activate().await.unwrap();
        // Chromium can open a cached former-default input after handoff.
        r.pulse.streams.insert("12".into(), "1".into());
        r.pulse.streams.insert("13".into(), "3".into());
        r.refresh().await.unwrap();
        assert_eq!(r.pulse.streams["12"], "2");
        assert_eq!(r.pulse.streams["13"], "3");
        // A manual change after observation wins, even back to the former input.
        r.pulse.streams.insert("12".into(), "1".into());
        r.refresh().await.unwrap();
        assert_eq!(r.pulse.streams["12"], "1");
        // An explicitly changed system default also prevents new automatic moves.
        r.pulse.default = "headset".into();
        r.pulse.streams.insert("14".into(), "1".into());
        r.refresh().await.unwrap();
        assert_eq!(r.pulse.streams["14"], "1");
        r.deactivate().await.unwrap();
        assert_eq!(r.pulse.default, "headset");
        assert_eq!(r.pulse.streams["12"], "1");
    }
    #[tokio::test]
    async fn manually_selected_remote_stays_muted() {
        let mut r = Route::new(
            Fake {
                default: SOURCE.into(),
                ..Fake::default()
            },
            None,
        );
        r.activate().await.unwrap();
        r.deactivate().await.unwrap();
        assert_eq!(r.pulse.default, SOURCE);
        assert!(r.pulse.muted);
    }
    #[tokio::test]
    async fn recovers_persisted_route_after_crash() {
        let dir = std::env::temp_dir().join(format!("lan-mic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("route.json");
        let mut r = Route::new(Fake::default(), Some(path.clone()));
        r.activate().await.unwrap();
        let mut restarted = Route::new(r.pulse, Some(path));
        restarted.recover().await.unwrap();
        assert_eq!(restarted.pulse.default, "local");
        assert_eq!(restarted.pulse.streams["10"], "1");
        assert!(restarted.pulse.muted);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
