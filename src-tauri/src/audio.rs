//! Audio engine. A single dedicated thread owns every WASAPI/COM object (MTA).
//! The rest of the app talks to it through `Cmd`s and receives snapshots through `Hooks`.
//!
//! - Sessions are grouped per executable ("app"), so Chrome's many sessions show as one row
//! - Peak meters are polled at ~30 Hz, but only while someone is watching (`SetMetering(true)`)
//! - New sessions of a known app inherit its remembered volume and mute

use crate::winutil::{foreground_pid, icon_data_url, process_path, product_name, stem_name, take_pwstr};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

pub const SYSTEM_KEY: &str = "#system";
const REFRESH_EVERY: Duration = Duration::from_millis(1000);
const METER_EVERY: Duration = Duration::from_millis(33);

// ================================================================ public data

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Stable id: lower-case executable file name (e.g. "chrome.exe"), or "#system".
    pub key: String,
    pub name: String,
    pub icon: Option<String>,
    pub volume: f32,
    pub muted: bool,
    /// At least one session is currently playing.
    pub active: bool,
    pub system: bool,
    /// Volume is stored and restored on next launch.
    pub remembered: bool,
    pub sessions: usize,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Mixer {
    pub devices: Vec<Device>,
    pub device_id: String,
    pub follow_default: bool,
    pub master_volume: f32,
    pub master_muted: bool,
    pub apps: Vec<AppInfo>,
    pub remember: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Levels {
    pub master: f32,
    pub apps: HashMap<String, f32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Osd {
    pub name: String,
    pub icon: Option<String>,
    pub volume: f32,
    pub muted: bool,
    /// false when the foreground app has no audio session.
    pub has_audio: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub volume: f32,
    pub muted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Memory {
    pub remember: bool,
    pub volumes: HashMap<String, Saved>,
}

impl Default for Memory {
    fn default() -> Self {
        Self { remember: true, volumes: HashMap::new() }
    }
}

pub enum Cmd {
    SetVolume(String, f32),
    SetMute(String, bool),
    SetMasterVolume(f32),
    SetMasterMute(bool),
    /// None = follow the Windows default output
    SelectDevice(Option<String>),
    SetMetering(bool),
    /// Change the foreground app's volume by a delta (hotkeys)
    NudgeForeground(f32),
    ToggleMuteForeground,
    SetRemember(bool),
    Forget(String),
    Refresh,
}

pub struct Hooks {
    pub mixer: Box<dyn Fn(&Mixer) + Send>,
    pub levels: Box<dyn Fn(&Levels) + Send>,
    pub osd: Box<dyn Fn(&Osd) + Send>,
    pub save: Box<dyn Fn(&Memory) + Send>,
}

// ================================================================ engine internals

struct Session {
    key: String,
    vol: ISimpleAudioVolume,
    meter: Option<IAudioMeterInformation>,
    active: bool,
}

struct Meta {
    name: String,
    icon: Option<String>,
    system: bool,
}

struct Endpoint {
    id: String,
    manager: IAudioSessionManager2,
    volume: IAudioEndpointVolume,
    meter: IAudioMeterInformation,
}

struct Engine {
    enumerator: IMMDeviceEnumerator,
    endpoint: Option<Endpoint>,
    chosen: Option<String>,
    devices: Vec<Device>,
    sessions: Vec<Session>,
    known_ids: HashSet<String>,
    meta: HashMap<String, Meta>,
    memory: Memory,
    memory_dirty: bool,
    metering: bool,
    last: Option<Mixer>,
    error: Option<String>,
}

/// Key for a process: lower-case executable name.
pub fn key_for_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_lowercase()
}

pub fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

impl Engine {
    fn new(memory: Memory) -> windows::core::Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        Ok(Self {
            enumerator,
            endpoint: None,
            chosen: None,
            devices: Vec::new(),
            sessions: Vec::new(),
            known_ids: HashSet::new(),
            meta: HashMap::new(),
            memory,
            memory_dirty: false,
            metering: false,
            last: None,
            error: None,
        })
    }

    fn list_devices(&self) -> windows::core::Result<(Vec<Device>, String)> {
        unsafe {
            let default_id = self
                .enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .and_then(|d| d.GetId())
                .map(take_pwstr)
                .unwrap_or_default();
            let coll = self.enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
            let mut out = Vec::new();
            for i in 0..coll.GetCount()? {
                let Ok(dev) = coll.Item(i) else { continue };
                let Ok(id) = dev.GetId().map(take_pwstr) else { continue };
                let name = dev
                    .OpenPropertyStore(STGM_READ)
                    .and_then(|ps| ps.GetValue(&PKEY_Device_FriendlyName))
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| "Audio device".into());
                out.push(Device { is_default: id == default_id, id, name });
            }
            out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
            Ok((out, default_id))
        }
    }

    fn open_endpoint(&self, id: &str) -> windows::core::Result<Endpoint> {
        unsafe {
            let wid: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
            let dev: IMMDevice = self.enumerator.GetDevice(windows::core::PCWSTR(wid.as_ptr()))?;
            Ok(Endpoint {
                id: id.to_string(),
                manager: dev.Activate(CLSCTX_ALL, None)?,
                volume: dev.Activate(CLSCTX_ALL, None)?,
                meter: dev.Activate(CLSCTX_ALL, None)?,
            })
        }
    }

    fn meta_for(&mut self, key: &str, pid: u32, path: Option<&str>, display: &str, system: bool) {
        if self.meta.contains_key(key) {
            return;
        }
        let meta = if system {
            Meta { name: "System sounds".into(), icon: None, system: true }
        } else {
            let name = path
                .and_then(product_name)
                .or_else(|| (!display.is_empty() && !display.starts_with('@')).then(|| display.to_string()))
                .or_else(|| path.map(stem_name))
                .unwrap_or_else(|| format!("Process {pid}"));
            Meta { name, icon: path.and_then(|p| icon_data_url(p, 64)), system: false }
        };
        self.meta.insert(key.to_string(), meta);
    }

    fn refresh(&mut self) {
        if let Err(e) = self.try_refresh() {
            self.error = Some(format!("Audio device unavailable ({})", e.message()));
            self.endpoint = None;
            self.sessions.clear();
        }
    }

    fn try_refresh(&mut self) -> windows::core::Result<()> {
        let (devices, default_id) = self.list_devices()?;
        let target = match &self.chosen {
            Some(id) if devices.iter().any(|d| &d.id == id) => id.clone(),
            _ => default_id.clone(),
        };
        self.devices = devices;
        if target.is_empty() {
            self.endpoint = None;
            self.sessions.clear();
            self.error = Some("No active output device".into());
            return Ok(());
        }
        if self.endpoint.as_ref().map(|e| e.id.as_str()) != Some(target.as_str()) {
            self.endpoint = Some(self.open_endpoint(&target)?);
            self.known_ids.clear();
        }
        self.error = None;

        let ep = self.endpoint.as_ref().unwrap();
        let en = unsafe { ep.manager.GetSessionEnumerator()? };
        let mut sessions = Vec::new();
        let mut fresh: Vec<(usize, String)> = Vec::new(); // (index into sessions, instance id)
        let mut seen_ids = HashSet::new();
        for i in 0..unsafe { en.GetCount()? } {
            let Ok(ctl) = (unsafe { en.GetSession(i) }) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let state = unsafe { ctl2.GetState() }.unwrap_or(AudioSessionStateExpired);
            if state == AudioSessionStateExpired {
                continue;
            }
            let Ok(vol) = ctl2.cast::<ISimpleAudioVolume>() else { continue };
            let id = unsafe { ctl2.GetSessionInstanceIdentifier() }.map(take_pwstr).unwrap_or_default();
            let pid = unsafe { ctl2.GetProcessId() }.unwrap_or(0);
            let system = unsafe { ctl2.IsSystemSoundsSession() } == S_OK;
            let path = if system { None } else { process_path(pid) };
            let key = if system {
                SYSTEM_KEY.to_string()
            } else {
                path.as_deref().map(key_for_path).unwrap_or_else(|| format!("pid:{pid}"))
            };
            let display = unsafe { ctl2.GetDisplayName() }.map(take_pwstr).unwrap_or_default();
            self.meta_for(&key, pid, path.as_deref(), &display, system);
            if !self.known_ids.contains(&id) {
                fresh.push((sessions.len(), id.clone()));
            }
            seen_ids.insert(id);
            sessions.push(Session { key, vol, meter: ctl2.cast().ok(), active: state == AudioSessionStateActive });
        }

        // New sessions: restore the remembered volume, or inherit from an already-known sibling of the same app
        // (e.g. Chrome opening a fresh audio stream must not jump back to 100 %).
        let fresh_idx: HashSet<usize> = fresh.iter().map(|(i, _)| *i).collect();
        let first_run = self.known_ids.is_empty();
        for (idx, id) in fresh {
            let key = sessions[idx].key.clone();
            let remembered = if self.memory.remember { self.memory.volumes.get(&key).copied() } else { None };
            let sibling = || {
                sessions
                    .iter()
                    .enumerate()
                    .find(|(j, s)| !fresh_idx.contains(j) && s.key == key)
                    .and_then(|(_, s)| unsafe {
                        Some(Saved { volume: s.vol.GetMasterVolume().ok()?, muted: s.vol.GetMute().ok()?.as_bool() })
                    })
            };
            // On the very first scan, don't override what the user already has playing.
            let target = if first_run { None } else { remembered.or_else(sibling) };
            if let Some(t) = target {
                unsafe {
                    let _ = sessions[idx].vol.SetMasterVolume(clamp01(t.volume), std::ptr::null());
                    let _ = sessions[idx].vol.SetMute(t.muted, std::ptr::null());
                }
            }
            self.known_ids.insert(id);
        }
        self.known_ids.retain(|id| seen_ids.contains(id));
        self.sessions = sessions;
        Ok(())
    }

    fn snapshot(&self) -> Mixer {
        let mut apps: Vec<AppInfo> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        for s in &self.sessions {
            if let Some(&i) = index.get(s.key.as_str()) {
                apps[i].sessions += 1;
                apps[i].active |= s.active;
                continue;
            }
            let (volume, muted) = unsafe {
                (s.vol.GetMasterVolume().unwrap_or(1.0), s.vol.GetMute().map(|b| b.as_bool()).unwrap_or(false))
            };
            let meta = self.meta.get(&s.key);
            index.insert(&s.key, apps.len());
            apps.push(AppInfo {
                key: s.key.clone(),
                name: meta.map(|m| m.name.clone()).unwrap_or_else(|| s.key.clone()),
                icon: meta.and_then(|m| m.icon.clone()),
                volume: (volume * 1000.0).round() / 1000.0,
                muted,
                active: s.active,
                system: meta.is_some_and(|m| m.system),
                remembered: self.memory.remember && self.memory.volumes.contains_key(&s.key),
                sessions: 1,
            });
        }
        // apps first (alphabetical), system sounds last
        apps.sort_by(|a, b| a.system.cmp(&b.system).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let (master_volume, master_muted) = self
            .endpoint
            .as_ref()
            .map(|e| unsafe {
                (e.volume.GetMasterVolumeLevelScalar().unwrap_or(0.0), e.volume.GetMute().map(|b| b.as_bool()).unwrap_or(false))
            })
            .unwrap_or((0.0, false));
        Mixer {
            devices: self.devices.clone(),
            device_id: self.endpoint.as_ref().map(|e| e.id.clone()).unwrap_or_default(),
            follow_default: self.chosen.is_none(),
            master_volume: (master_volume * 1000.0).round() / 1000.0,
            master_muted,
            apps,
            remember: self.memory.remember,
            error: self.error.clone(),
        }
    }

    fn levels(&self) -> Levels {
        let mut apps: HashMap<String, f32> = HashMap::new();
        for s in &self.sessions {
            let p = s.meter.as_ref().and_then(|m| unsafe { m.GetPeakValue().ok() }).unwrap_or(0.0);
            let e = apps.entry(s.key.clone()).or_insert(0.0);
            *e = e.max(p);
        }
        let master = self.endpoint.as_ref().and_then(|e| unsafe { e.meter.GetPeakValue().ok() }).unwrap_or(0.0);
        Levels { master, apps }
    }

    fn app_state(&self, key: &str) -> Option<Saved> {
        let s = self.sessions.iter().find(|s| s.key == key)?;
        unsafe { Some(Saved { volume: s.vol.GetMasterVolume().ok()?, muted: s.vol.GetMute().ok()?.as_bool() }) }
    }

    fn apply(&mut self, key: &str, volume: Option<f32>, muted: Option<bool>) {
        for s in self.sessions.iter().filter(|s| s.key == key) {
            unsafe {
                if let Some(v) = volume {
                    let _ = s.vol.SetMasterVolume(clamp01(v), std::ptr::null());
                }
                if let Some(m) = muted {
                    let _ = s.vol.SetMute(m, std::ptr::null());
                }
            }
        }
        if self.memory.remember && key != SYSTEM_KEY {
            if let Some(state) = self.app_state(key) {
                self.memory.volumes.insert(key.to_string(), state);
                self.memory_dirty = true;
            }
        }
    }

    fn foreground_osd(&mut self, delta: Option<f32>, toggle_mute: bool) -> Osd {
        let path = foreground_pid().and_then(process_path);
        let Some(path) = path else {
            return Osd { name: "Unknown app".into(), icon: None, volume: 0.0, muted: false, has_audio: false };
        };
        let key = key_for_path(&path);
        let Some(cur) = self.app_state(&key) else {
            let name = product_name(&path).unwrap_or_else(|| stem_name(&path));
            return Osd { name, icon: icon_data_url(&path, 64), volume: 0.0, muted: false, has_audio: false };
        };
        let volume = delta.map(|d| clamp01(((cur.volume + d) * 100.0).round() / 100.0));
        let muted = if toggle_mute { Some(!cur.muted) } else if delta.is_some_and(|d| d > 0.0) { Some(false) } else { None };
        self.apply(&key, volume, muted);
        let now = self.app_state(&key).unwrap_or(cur);
        let meta = self.meta.get(&key);
        Osd {
            name: meta.map(|m| m.name.clone()).unwrap_or_else(|| stem_name(&path)),
            icon: meta.and_then(|m| m.icon.clone()),
            volume: now.volume,
            muted: now.muted,
            has_audio: true,
        }
    }
}

/// Run the engine on the current thread until the command channel closes.
pub fn run(rx: Receiver<Cmd>, memory: Memory, hooks: Hooks) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut eng = match Engine::new(memory) {
        Ok(e) => e,
        Err(e) => {
            (hooks.mixer)(&Mixer { error: Some(format!("Windows audio is unavailable: {}", e.message())), ..Default::default() });
            return;
        }
    };
    eng.refresh();
    let mut last_refresh = Instant::now();
    let mut publish = true;

    loop {
        let wait = if eng.metering { METER_EVERY } else { REFRESH_EVERY };
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                let mut cmds = vec![cmd];
                cmds.extend(rx.try_iter()); // coalesce bursts (slider drags)
                for cmd in cmds {
                    match cmd {
                        Cmd::SetVolume(k, v) => eng.apply(&k, Some(v), Some(false)),
                        Cmd::SetMute(k, m) => eng.apply(&k, None, Some(m)),
                        Cmd::SetMasterVolume(v) => {
                            if let Some(e) = &eng.endpoint {
                                unsafe {
                                    let _ = e.volume.SetMasterVolumeLevelScalar(clamp01(v), std::ptr::null());
                                    let _ = e.volume.SetMute(false, std::ptr::null());
                                }
                            }
                        }
                        Cmd::SetMasterMute(m) => {
                            if let Some(e) = &eng.endpoint {
                                unsafe {
                                    let _ = e.volume.SetMute(m, std::ptr::null());
                                }
                            }
                        }
                        Cmd::SelectDevice(id) => {
                            eng.chosen = id;
                            eng.refresh();
                            last_refresh = Instant::now();
                        }
                        Cmd::SetMetering(on) => eng.metering = on,
                        Cmd::NudgeForeground(d) => (hooks.osd)(&eng.foreground_osd(Some(d), false)),
                        Cmd::ToggleMuteForeground => (hooks.osd)(&eng.foreground_osd(None, true)),
                        Cmd::SetRemember(on) => {
                            eng.memory.remember = on;
                            eng.memory_dirty = true;
                        }
                        Cmd::Forget(k) => {
                            eng.memory.volumes.remove(&k);
                            eng.memory_dirty = true;
                        }
                        Cmd::Refresh => {
                            eng.refresh();
                            last_refresh = Instant::now();
                        }
                    }
                }
                publish = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if eng.metering {
            (hooks.levels)(&eng.levels());
        }
        if last_refresh.elapsed() >= REFRESH_EVERY {
            eng.refresh();
            last_refresh = Instant::now();
            publish = true;
        }
        if publish {
            let snap = eng.snapshot();
            if eng.last.as_ref() != Some(&snap) {
                (hooks.mixer)(&snap);
                eng.last = Some(snap);
            }
            publish = false;
        }
        if eng.memory_dirty {
            (hooks.save)(&eng.memory);
            eng.memory_dirty = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_clamp() {
        assert_eq!(key_for_path(r"C:\Program Files\Google\Chrome\Application\Chrome.exe"), "chrome.exe");
        assert_eq!(clamp01(1.4), 1.0);
        assert_eq!(clamp01(-0.1), 0.0);
        assert_eq!(clamp01(f32::NAN), 0.0);
    }

    #[test]
    fn memory_defaults_to_remember() {
        let m: Memory = serde_json::from_str("{}").unwrap();
        assert!(m.remember);
        assert!(m.volumes.is_empty());
    }

    /// Talks to the real Windows audio stack: lists devices and takes a snapshot.
    #[test]
    fn engine_snapshot_on_this_machine() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut eng = Engine::new(Memory::default()).expect("MMDeviceEnumerator");
        eng.refresh();
        let snap = eng.snapshot();
        if snap.devices.is_empty() {
            return; // CI machines often have no audio device
        }
        assert!(snap.devices.iter().any(|d| d.is_default));
        assert!(!snap.device_id.is_empty());
        assert!((0.0..=1.0).contains(&snap.master_volume));
        let _ = eng.levels();
    }
}
