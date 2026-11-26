//! Browser tabs (phase 2).
//!
//! ```text
//! extension ──native messaging (stdio)──▶ mixdeck.exe in bridge mode ──TCP 127.0.0.1 + token──▶ Mixdeck app
//! ```
//! Chrome/Edge start the native host themselves, with the extension origin as argv[1]. Our single executable
//! detects that and runs [`run_bridge`] instead of the UI. The bridge relays JSON both ways and tells the app
//! which browser it belongs to (from its parent process), so tabs land under the right app row.

use crate::winutil::{process_path, product_name, stem_name};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const HOST_NAME: &str = "id.wishnu.mixdeck";
pub const EXTENSION_ID: &str = "pkjnmnolfphodjljfffplljmkgeppgjk";
const APP_DIR: &str = "id.wishnu.mixdeck"; // = Tauri app_data_dir name (bundle identifier)
const MAX_MSG: usize = 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub fav_icon_url: String,
    #[serde(default)]
    pub audible: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default = "one")]
    pub volume: f32,
    #[serde(default)]
    pub active: bool,
}
fn one() -> f32 {
    1.0
}

/// One connected browser profile and its tabs.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabs {
    pub conn: u64,
    /// Executable key, matching the app row (e.g. "chrome.exe").
    pub browser: String,
    pub name: String,
    pub tabs: Vec<Tab>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct BridgeInfo {
    pub port: u16,
    pub token: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Hello {
    hello: String,
    browser: String,
    name: String,
}

#[derive(Deserialize)]
struct FromExtension {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    tabs: Vec<Tab>,
}

// ================================================================ Chrome native-messaging framing

/// Read one message: 4-byte little-endian length + UTF-8 JSON. `Ok(None)` on clean EOF.
pub fn read_msg(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_MSG {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

pub fn write_msg(w: &mut impl Write, json: &[u8]) -> io::Result<()> {
    w.write_all(&(json.len() as u32).to_le_bytes())?;
    w.write_all(json)?;
    w.flush()
}

// ================================================================ app side: hub

struct Conn {
    browser: String,
    name: String,
    tabs: Vec<Tab>,
    writer: TcpStream,
}

#[derive(Clone)]
pub struct TabHub {
    conns: Arc<Mutex<HashMap<u64, Conn>>>,
}

type OnChange = Arc<dyn Fn(Vec<BrowserTabs>) + Send + Sync>;

impl TabHub {
    pub fn snapshot(&self) -> Vec<BrowserTabs> {
        let conns = self.conns.lock().unwrap();
        let mut v: Vec<BrowserTabs> = conns
            .iter()
            .map(|(id, c)| BrowserTabs { conn: *id, browser: c.browser.clone(), name: c.name.clone(), tabs: c.tabs.clone() })
            .collect();
        v.sort_by_key(|b| b.conn);
        v
    }

    /// Send a command (JSON object) to one browser connection.
    pub fn send(&self, conn: u64, msg: &serde_json::Value) -> bool {
        let mut conns = self.conns.lock().unwrap();
        let Some(c) = conns.get_mut(&conn) else { return false };
        let mut line = msg.to_string();
        line.push('\n');
        c.writer.write_all(line.as_bytes()).is_ok()
    }
}

fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..4)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
            h.write_u32(i ^ std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(APP_DIR)
}

/// Start the local server and write `bridge.json` (port + token) for bridges to find it.
pub fn start_hub(dir: &Path, on_change: impl Fn(Vec<BrowserTabs>) + Send + Sync + 'static) -> io::Result<TabHub> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let info = BridgeInfo { port: listener.local_addr()?.port(), token: random_token() };
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("bridge.json"), serde_json::to_vec(&info).unwrap())?;

    let hub = TabHub { conns: Arc::new(Mutex::new(HashMap::new())) };
    let on_change: OnChange = Arc::new(on_change);
    let (h, token) = (hub.clone(), info.token);
    std::thread::Builder::new().name("tab-hub".into()).spawn(move || {
        let mut next = 1u64;
        for stream in listener.incoming().flatten() {
            let id = next;
            next += 1;
            let (h, token, cb) = (h.clone(), token.clone(), on_change.clone());
            std::thread::spawn(move || serve(id, stream, &token, &h, &cb));
        }
    })?;
    Ok(hub)
}

fn serve(id: u64, stream: TcpStream, token: &str, hub: &TabHub, on_change: &OnChange) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(writer) = stream.try_clone() else { return };
    let mut lines = BufReader::new(stream).lines();
    let Some(Ok(first)) = lines.next() else { return };
    let Ok(hello) = serde_json::from_str::<Hello>(&first) else { return };
    if !same(&hello.hello, token) {
        return; // not our bridge
    }
    let _ = writer.set_read_timeout(None);
    if let Ok(s) = writer.try_clone() {
        let _ = s.set_read_timeout(None);
    }
    hub.conns.lock().unwrap().insert(id, Conn { browser: hello.browser, name: hello.name, tabs: Vec::new(), writer });
    on_change(hub.snapshot());

    // after the handshake, reads may block indefinitely
    for line in lines {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<FromExtension>(&line) else { continue };
        if msg.kind == "tabs" {
            let changed = {
                let mut conns = hub.conns.lock().unwrap();
                match conns.get_mut(&id) {
                    Some(c) if c.tabs != msg.tabs => {
                        c.tabs = msg.tabs;
                        true
                    }
                    _ => false,
                }
            };
            if changed {
                on_change(hub.snapshot());
            }
        }
    }
    hub.conns.lock().unwrap().remove(&id);
    on_change(hub.snapshot());
}

// ================================================================ native host registration

/// Write the host manifest and register it for Chrome, Edge, Brave and Chromium (per user, no admin).
pub fn register_native_host(dir: &Path) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let manifest = serde_json::json!({
        "name": HOST_NAME,
        "description": "Mixdeck tab-volume bridge",
        "path": exe,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("native-host.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).map_err(|e| e.to_string())?;
    for browser in [r"Google\Chrome", r"Microsoft\Edge", r"BraveSoftware\Brave-Browser", "Chromium"] {
        let key = format!(r"Software\{browser}\NativeMessagingHosts\{HOST_NAME}");
        windows_registry::CURRENT_USER
            .create(&key)
            .and_then(|k| k.set_string("", path.to_string_lossy().as_ref()))
            .map_err(|e| format!("{key}: {e}"))?;
    }
    Ok(path)
}

// ================================================================ bridge mode

/// True when Chrome/Edge launched us as a native-messaging host.
pub fn is_bridge_launch() -> Option<String> {
    std::env::args().skip(1).find(|a| a.starts_with("chrome-extension://"))
}

/// pid → parent pid for every running process.
fn parent_map() -> HashMap<u32, u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut map = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return map };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                map.insert(e.th32ProcessID, e.th32ParentProcessID);
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    map
}

/// The browser that launched us. On Windows, Chrome starts native hosts through `cmd.exe /c`, so the direct
/// parent is the shell: walk up past shell/console processes to the real browser.
fn browser_process_path() -> Option<String> {
    const SKIP: &[&str] = &["cmd.exe", "conhost.exe", "powershell.exe", "pwsh.exe"];
    let parents = parent_map();
    let mut pid = std::process::id();
    for _ in 0..6 {
        pid = *parents.get(&pid)?;
        let path = process_path(pid)?;
        if !SKIP.contains(&crate::audio::key_for_path(&path).as_str()) {
            return Some(path);
        }
    }
    None
}

/// Relay between the browser (stdio) and the Mixdeck app (TCP). Never returns: exits when the browser
/// closes stdin. While the app isn't running it keeps retrying, and the latest tab list is replayed on
/// connect.
pub fn run_bridge(_origin: &str) -> ! {
    let parent = browser_process_path();
    let browser = parent.as_deref().map(crate::audio::key_for_path).unwrap_or_else(|| "chrome.exe".into());
    let name = parent.as_deref().map(|p| product_name(p).unwrap_or_else(|| stem_name(p))).unwrap_or_else(|| "Browser".into());

    let latest_tabs: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let tcp: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    let stdout = Arc::new(Mutex::new(io::stdout()));

    // browser → app
    {
        let (latest, tcp) = (latest_tabs.clone(), tcp.clone());
        std::thread::spawn(move || {
            let mut stdin = io::stdin().lock();
            loop {
                match read_msg(&mut stdin) {
                    Ok(Some(buf)) => {
                        let line = String::from_utf8_lossy(&buf).replace('\n', " ");
                        if line.contains("\"tabs\"") {
                            *latest.lock().unwrap() = Some(line.clone());
                        }
                        let mut guard = tcp.lock().unwrap();
                        if let Some(s) = guard.as_mut() {
                            if s.write_all(format!("{line}\n").as_bytes()).is_err() {
                                *guard = None;
                            }
                        }
                    }
                    _ => std::process::exit(0), // browser went away
                }
            }
        });
    }

    // app → browser, with reconnects
    loop {
        let info: Option<BridgeInfo> =
            std::fs::read(app_dir().join("bridge.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let stream = info.as_ref().and_then(|i| TcpStream::connect(("127.0.0.1", i.port)).ok());
        if let (Some(info), Some(mut stream)) = (info, stream) {
            let hello = serde_json::to_string(&Hello { hello: info.token, browser: browser.clone(), name: name.clone() }).unwrap();
            let mut ok = stream.write_all(format!("{hello}\n").as_bytes()).is_ok();
            if let Some(t) = latest_tabs.lock().unwrap().clone() {
                ok &= stream.write_all(format!("{t}\n").as_bytes()).is_ok();
            }
            if ok {
                if let Ok(reader) = stream.try_clone() {
                    *tcp.lock().unwrap() = Some(stream);
                    // ask the extension for a fresh list now that the app is listening
                    let _ = write_msg(&mut *stdout.lock().unwrap(), br#"{"type":"refresh"}"#);
                    for line in BufReader::new(reader).lines() {
                        let Ok(line) = line else { break };
                        if write_msg(&mut *stdout.lock().unwrap(), line.as_bytes()).is_err() {
                            std::process::exit(0);
                        }
                    }
                    *tcp.lock().unwrap() = None;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_roundtrip() {
        let mut buf = Vec::new();
        write_msg(&mut buf, br#"{"type":"refresh"}"#).unwrap();
        assert_eq!(&buf[..4], &18u32.to_le_bytes());
        let mut r = &buf[..];
        assert_eq!(read_msg(&mut r).unwrap().unwrap(), br#"{"type":"refresh"}"#);
        assert!(read_msg(&mut r).unwrap().is_none(), "clean EOF");
    }

    #[test]
    fn rejects_oversized_messages() {
        let mut r = &(MAX_MSG as u32 + 1).to_le_bytes()[..];
        assert!(read_msg(&mut r).is_err());
    }

    #[test]
    fn tab_json_from_extension() {
        let t: Tab = serde_json::from_str(r#"{"id":7,"title":"Lo-fi","host":"youtube.com","favIconUrl":"x","audible":true,"muted":false,"volume":0.4}"#).unwrap();
        assert_eq!((t.id, t.audible, t.volume), (7, true, 0.4));
        let t: Tab = serde_json::from_str(r#"{"id":1,"title":"t"}"#).unwrap();
        assert_eq!(t.volume, 1.0, "missing volume means untouched");
    }

    /// Full path: hub ← fake bridge over TCP. Wrong tokens are refused; tab updates arrive; commands go out.
    #[test]
    fn hub_handshake_updates_and_commands() {
        let dir = std::env::temp_dir().join(format!("mixdeck-test-{}", std::process::id()));
        let seen: Arc<Mutex<Vec<Vec<BrowserTabs>>>> = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let hub = start_hub(&dir, move |v| s2.lock().unwrap().push(v)).unwrap();
        let info: BridgeInfo = serde_json::from_slice(&std::fs::read(dir.join("bridge.json")).unwrap()).unwrap();

        // intruder with a bad token is ignored
        let mut bad = TcpStream::connect(("127.0.0.1", info.port)).unwrap();
        bad.write_all(b"{\"hello\":\"nope\",\"browser\":\"evil.exe\",\"name\":\"x\"}\n").unwrap();

        let mut good = TcpStream::connect(("127.0.0.1", info.port)).unwrap();
        let hello = serde_json::to_string(&Hello { hello: info.token.clone(), browser: "chrome.exe".into(), name: "Google Chrome".into() }).unwrap();
        good.write_all(format!("{hello}\n").as_bytes()).unwrap();
        good.write_all(b"{\"type\":\"tabs\",\"tabs\":[{\"id\":3,\"title\":\"Music\",\"audible\":true,\"volume\":0.5}]}\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let snap = hub.snapshot();
        assert_eq!(snap.len(), 1, "only the authenticated bridge is listed");
        assert_eq!(snap[0].browser, "chrome.exe");
        assert_eq!(snap[0].tabs[0].title, "Music");

        assert!(hub.send(snap[0].conn, &serde_json::json!({"type":"setVolume","tabId":3,"volume":0.2})));
        let mut line = String::new();
        BufReader::new(good.try_clone().unwrap()).read_line(&mut line).unwrap();
        assert!(line.contains("\"setVolume\"") && line.ends_with('\n'));

        drop(good);
        std::thread::sleep(Duration::from_millis(300));
        assert!(hub.snapshot().is_empty(), "disconnect removes the browser");
        assert!(seen.lock().unwrap().len() >= 3);
        let _ = std::fs::remove_dir_all(dir);
    }
}
