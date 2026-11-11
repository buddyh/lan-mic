//! Feed received PCM into BlackHole without changing the Mac's speakers.
use anyhow::{bail, ensure, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    ffi::{c_char, c_void},
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}
const fn cc(b: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*b)
}
#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectGetPropertyDataSize(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> bool;
    fn CFRelease(object: *const c_void);
}
fn address(selector: u32) -> Address {
    Address {
        selector,
        scope: cc(b"glob"),
        element: 0,
    }
}
fn input() -> Result<u32> {
    let mut value = 0u32;
    let mut size = 4;
    let result = unsafe {
        AudioObjectGetPropertyData(
            1,
            &address(cc(b"dIn ")),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut u32).cast(),
        )
    };
    ensure!(result == 0, "Read default Mac input: {result}");
    Ok(value)
}
fn set_input(value: u32) -> Result<()> {
    let result = unsafe {
        AudioObjectSetPropertyData(
            1,
            &address(cc(b"dIn ")),
            0,
            std::ptr::null(),
            4,
            (&value as *const u32).cast(),
        )
    };
    ensure!(result == 0, "Set default Mac input: {result}");
    Ok(())
}
fn uid(id: u32) -> Result<String> {
    let mut value = std::ptr::null::<c_void>();
    let mut size = std::mem::size_of_val(&value) as u32;
    let result = unsafe {
        AudioObjectGetPropertyData(
            id,
            &address(cc(b"uid ")),
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut *const c_void).cast(),
        )
    };
    ensure!(
        result == 0 && !value.is_null(),
        "Read audio device UID: {result}"
    );
    let mut bytes = [0i8; 1024];
    let converted =
        unsafe { CFStringGetCString(value, bytes.as_mut_ptr(), bytes.len() as isize, 0x08000100) };
    unsafe { CFRelease(value) };
    ensure!(converted, "Audio device UID is too long");
    Ok(unsafe { std::ffi::CStr::from_ptr(bytes.as_ptr()) }
        .to_str()?
        .to_owned())
}
fn device_by_uid(wanted: &str) -> Result<u32> {
    let mut size = 0;
    let addr = address(cc(b"dev#"));
    let result =
        unsafe { AudioObjectGetPropertyDataSize(1, &addr, 0, std::ptr::null(), &mut size) };
    ensure!(result == 0, "List Mac audio devices: {result}");
    let mut devices = vec![0u32; size as usize / 4];
    let result = unsafe {
        AudioObjectGetPropertyData(
            1,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            devices.as_mut_ptr().cast(),
        )
    };
    ensure!(result == 0, "List Mac audio devices: {result}");
    devices
        .into_iter()
        .find(|id| uid(*id).is_ok_and(|u| u == wanted))
        .context("Previous microphone is unavailable")
}
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    previous: Option<String>,
    remote: Option<String>,
}
struct InputRoute {
    path: PathBuf,
    saved: Saved,
}
impl InputRoute {
    fn new(path: PathBuf) -> Result<Self> {
        let saved = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?)?
        } else {
            Saved::default()
        };
        let mut route = Self { path, saved };
        route.restore()?;
        Ok(route)
    }
    fn activate(&mut self, remote: u32) -> Result<()> {
        let current = input()?;
        let remote_uid = uid(remote)?;
        self.saved = Saved {
            previous: if current == remote {
                None
            } else {
                Some(uid(current)?)
            },
            remote: Some(remote_uid),
        };
        crate::atomic_json(&self.path, &self.saved)?;
        set_input(remote)
    }
    fn restore(&mut self) -> Result<()> {
        if let (Some(previous), Some(remote)) = (&self.saved.previous, &self.saved.remote) {
            // Restore only our own selection. Keep a user's manual change.
            if uid(input()?).is_ok_and(|u| u == *remote) {
                if let Ok(id) = device_by_uid(previous) {
                    set_input(id)?;
                }
            }
        }
        self.saved = Saved::default();
        crate::atomic_json(&self.path, &self.saved)
    }
}
pub struct Incoming {
    stream: Option<cpal::Stream>,
    queue: Arc<Mutex<VecDeque<f32>>>,
    route: InputRoute,
    pub active: bool,
}
impl Incoming {
    pub fn new() -> Result<Self> {
        Ok(Self {
            stream: None,
            queue: Arc::new(Mutex::new(VecDeque::new())),
            route: InputRoute::new(crate::config::state_dir().join("input-route.json"))?,
            active: false,
        })
    }
    pub fn available() -> bool {
        blackhole().is_ok()
    }
    pub fn push(&mut self, pcm: &[u8]) -> Result<()> {
        if !self.active {
            let device = blackhole()?;
            let id = device_by_uid(device.id()?.id())?;
            let format = device.default_output_config()?;
            ensure!(
                format.sample_format() == SampleFormat::F32,
                "BlackHole must support float audio"
            );
            let config = cpal::StreamConfig {
                channels: 2,
                sample_rate: 48000,
                buffer_size: cpal::BufferSize::Default,
            };
            let queue = self.queue.clone();
            let stream = device.build_output_stream(
                config,
                move |data: &mut [f32], _: &_| {
                    if let Ok(mut q) = queue.try_lock() {
                        for frame in data.chunks_mut(2) {
                            let sample = q.pop_front().unwrap_or(0.0);
                            frame.fill(sample);
                        }
                    } else {
                        data.fill(0.0);
                    }
                },
                |error| eprintln!("BlackHole playback: {error}"),
                None,
            )?;
            stream.play()?;
            if let Err(error) = self.route.activate(id) {
                let _ = self.route.restore();
                return Err(error);
            }
            self.stream = Some(stream);
            self.active = true;
        }
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| anyhow::anyhow!("Mac audio queue failed"))?;
        ensure!(
            queue.len() + pcm.len() / 2 <= 9600,
            "Mac audio playback stalled"
        );
        queue.extend(
            pcm.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
        );
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        if !self.active && self.stream.is_none() {
            return Ok(());
        }
        self.stream = None;
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
        self.active = false;
        self.route.restore()
    }
}
impl Drop for Incoming {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn blackhole() -> Result<cpal::Device> {
    for device in cpal::default_host().output_devices()? {
        if device.description()?.name() == "BlackHole 2ch" {
            return Ok(device);
        }
    }
    bail!("Install BlackHole 2ch and restart the Mac to receive a microphone")
}
