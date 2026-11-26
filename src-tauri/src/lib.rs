pub mod audio;
pub mod tabs;
pub mod winutil;

use audio::{Cmd, Hooks, Memory, Mixer};
use tabs::{BrowserTabs, TabHub};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState};

const MARGIN: i32 = 12;
const OSD_MS: u64 = 1400;

struct AppState {
    tx: Mutex<Sender<Cmd>>,
    last: Arc<Mutex<Mixer>>,
    hidden_at: Mutex<Instant>,
    tabs: Mutex<Option<TabHub>>,
    browser_status: Mutex<BrowserStatus>,
}

#[derive(serde::Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BrowserStatus {
    /// Native-messaging host registered for Chrome/Edge/Brave.
    registered: bool,
    error: Option<String>,
    extension_id: &'static str,
}

impl AppState {
    fn send(&self, cmd: Cmd) {
        let _ = self.tx.lock().unwrap().send(cmd);
    }
}

fn memory_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&dir);
    dir.join("mixdeck.json")
}

// ---------------------------------------------------------------- window placement

/// Anchor a window to the bottom-right of the work area of the monitor at `point` (or the primary one).
fn place_bottom_right(win: &WebviewWindow, point: Option<PhysicalPosition<f64>>) {
    let monitor = point
        .and_then(|p| win.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, win.outer_size()) else { return };
    let wa = m.work_area();
    let scale = m.scale_factor();
    let margin = (MARGIN as f64 * scale) as i32;
    let x = wa.position.x + wa.size.width as i32 - size.width as i32 - margin;
    let y = wa.position.y + wa.size.height as i32 - size.height as i32 - margin;
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

fn place_bottom_center(win: &WebviewWindow, point: Option<PhysicalPosition<f64>>) {
    let monitor = point
        .and_then(|p| win.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());
    let (Some(m), Ok(size)) = (monitor, win.outer_size()) else { return };
    let wa = m.work_area();
    let x = wa.position.x + (wa.size.width as i32 - size.width as i32) / 2;
    let y = wa.position.y + wa.size.height as i32 - size.height as i32 - (48.0 * m.scale_factor()) as i32;
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Windows 11 look: Mica backdrop (Acrylic fallback) and rounded corners.
fn style_window(win: &WebviewWindow, rounded: bool) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
        if window_vibrancy::apply_mica(win, None).is_err() {
            let _ = window_vibrancy::apply_acrylic(win, Some((20, 20, 24, 160)));
        }
        if rounded {
            if let Ok(h) = win.window_handle() {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    let hwnd = HWND(w.hwnd.get() as *mut core::ffi::c_void);
                    let pref = DWMWCP_ROUND;
                    unsafe {
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &pref as *const _ as *const core::ffi::c_void,
                            std::mem::size_of_val(&pref) as u32,
                        );
                    }
                }
            }
        }
    }
}

fn show_flyout(app: &AppHandle, point: Option<PhysicalPosition<f64>>) {
    let Some(win) = app.get_webview_window("main") else { return };
    place_bottom_right(&win, point);
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    app.state::<AppState>().send(Cmd::SetMetering(true));
    let _ = app.emit_to("main", "shown", ());
}

fn hide_flyout(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    let st = app.state::<AppState>();
    *st.hidden_at.lock().unwrap() = Instant::now();
    st.send(Cmd::SetMetering(false));
}

fn toggle_flyout(app: &AppHandle, point: Option<PhysicalPosition<f64>>) {
    let Some(win) = app.get_webview_window("main") else { return };
    // Clicking the tray while the flyout is open first blurs it (which hides it); don't reopen right away.
    let just_hidden = app.state::<AppState>().hidden_at.lock().unwrap().elapsed() < Duration::from_millis(300);
    if win.is_visible().unwrap_or(false) {
        hide_flyout(app);
    } else if !just_hidden {
        show_flyout(app, point);
    }
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_mixer(state: tauri::State<'_, AppState>) -> Mixer {
    state.last.lock().unwrap().clone()
}

#[tauri::command]
fn set_volume(key: String, volume: f32, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SetVolume(key, volume));
}

#[tauri::command]
fn set_mute(key: String, muted: bool, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SetMute(key, muted));
}

#[tauri::command]
fn set_master_volume(volume: f32, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SetMasterVolume(volume));
}

#[tauri::command]
fn set_master_mute(muted: bool, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SetMasterMute(muted));
}

#[tauri::command]
fn select_device(id: Option<String>, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SelectDevice(id));
}

#[tauri::command]
fn set_remember(on: bool, state: tauri::State<'_, AppState>) {
    state.send(Cmd::SetRemember(on));
}

#[tauri::command]
fn forget(key: String, state: tauri::State<'_, AppState>) {
    state.send(Cmd::Forget(key));
}

/// Resize the flyout to fit its content, keeping it anchored to the bottom-right.
#[tauri::command]
fn fit_window(window: WebviewWindow, height: f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let h = (height.clamp(160.0, 680.0) * scale).round() as u32;
    // Compare against the *inner* size: that's what set_size sets. Comparing the outer size (which includes
    // invisible borders) never matched and caused an endless resize → ResizeObserver → resize loop.
    let Ok(inner) = window.inner_size() else { return };
    if inner.height.abs_diff(h) > 1 {
        let _ = window.set_size(PhysicalSize::new(inner.width, h));
        place_bottom_right(&window, None);
    }
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    hide_flyout(&app);
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    Ok(al.is_enabled().unwrap_or(on))
}

#[tauri::command]
fn open_sound_settings() {
    let _ = tauri_plugin_opener::open_url("ms-settings:apps-volume", None::<&str>);
}

#[tauri::command]
fn get_tabs(state: tauri::State<'_, AppState>) -> Vec<BrowserTabs> {
    state.tabs.lock().unwrap().as_ref().map(|h| h.snapshot()).unwrap_or_default()
}

#[tauri::command]
fn set_tab_volume(conn: u64, tab_id: i64, volume: f32, state: tauri::State<'_, AppState>) {
    if let Some(h) = state.tabs.lock().unwrap().as_ref() {
        h.send(conn, &serde_json::json!({ "type": "setVolume", "tabId": tab_id, "volume": audio::clamp01(volume) }));
    }
}

#[tauri::command]
fn set_tab_mute(conn: u64, tab_id: i64, muted: bool, state: tauri::State<'_, AppState>) {
    if let Some(h) = state.tabs.lock().unwrap().as_ref() {
        h.send(conn, &serde_json::json!({ "type": "setMute", "tabId": tab_id, "muted": muted }));
    }
}

#[tauri::command]
fn get_browser_status(state: tauri::State<'_, AppState>) -> BrowserStatus {
    state.browser_status.lock().unwrap().clone()
}

/// Show the bundled extension folder in Explorer (for "Load unpacked" in chrome://extensions).
#[tauri::command]
fn open_extension_folder(app: AppHandle) -> Result<String, String> {
    let bundled = app.path().resource_dir().map(|d| d.join("extension")).ok().filter(|p| p.join("manifest.json").exists());
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("extension");
    let dir = bundled.unwrap_or(dev);
    let dir = dir.canonicalize().unwrap_or(dir);
    tauri_plugin_opener::reveal_item_in_dir(dir.join("manifest.json")).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().trim_start_matches(r"\\?\").to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------- setup

fn shortcut_handler(app: &AppHandle, sc: &Shortcut, event: ShortcutEvent) {
    if event.state() != ShortcutState::Pressed {
        return;
    }
    let st = app.state::<AppState>();
    let ca = Modifiers::CONTROL | Modifiers::ALT;
    if sc == &Shortcut::new(Some(ca), Code::ArrowUp) {
        st.send(Cmd::NudgeForeground(0.05));
    } else if sc == &Shortcut::new(Some(ca), Code::ArrowDown) {
        st.send(Cmd::NudgeForeground(-0.05));
    } else if sc == &Shortcut::new(Some(ca), Code::KeyM) {
        st.send(Cmd::ToggleMuteForeground);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_minimized = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_flyout(app, None)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(shortcut_handler).build())
        .setup(move |app| {
            let handle = app.handle().clone();

            // ---- audio engine thread
            let mem_path = memory_path(&handle);
            let memory: Memory = std::fs::read_to_string(&mem_path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let (tx, rx) = mpsc::channel();
            let last = Arc::new(Mutex::new(Mixer::default()));
            let osd_gen = Arc::new(AtomicU64::new(0));
            app.manage(AppState { tx: Mutex::new(tx), last: last.clone(), hidden_at: Mutex::new(Instant::now() - Duration::from_secs(5)), tabs: Mutex::new(None), browser_status: Mutex::new(BrowserStatus::default()) });

            // ---- browser tabs (phase 2): native host + local hub
            {
                let dir = handle.path().app_data_dir().unwrap_or_else(|_| tabs::app_dir());
                let st = handle.state::<AppState>();
                let reg = tabs::register_native_host(&dir);
                *st.browser_status.lock().unwrap() =
                    BrowserStatus { registered: reg.is_ok(), error: reg.err(), extension_id: tabs::EXTENSION_ID };
                let h = handle.clone();
                let trace = std::env::var("MIXDECK_TRACE").is_ok();
                match tabs::start_hub(&dir, move |snap| {
                    if trace {
                        eprintln!("tabs {}", serde_json::to_string(&snap).unwrap_or_default());
                    }
                    let _ = h.emit("tabs", snap);
                }) {
                    Ok(hub) => *st.tabs.lock().unwrap() = Some(hub),
                    Err(e) => st.browser_status.lock().unwrap().error = Some(format!("tab hub: {e}")),
                }
            }

            let (h1, h2, h3) = (handle.clone(), handle.clone(), handle.clone());
            std::thread::Builder::new().name("audio".into()).spawn(move || {
                audio::run(rx, memory, Hooks {
                    mixer: Box::new(move |m| {
                        *last.lock().unwrap() = m.clone();
                        let _ = h1.emit("mixer", m);
                    }),
                    levels: Box::new(move |l| {
                        let _ = h2.emit_to("main", "levels", l);
                    }),
                    osd: Box::new(move |o| {
                        let Some(win) = h3.get_webview_window("osd") else { return };
                        let _ = h3.emit_to("osd", "osd", o);
                        place_bottom_center(&win, h3.cursor_position().ok());
                        let _ = win.show();
                        let ticket = osd_gen.fetch_add(1, Ordering::SeqCst) + 1;
                        let (g, w) = (osd_gen.clone(), win.clone());
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(OSD_MS));
                            if g.load(Ordering::SeqCst) == ticket {
                                let _ = w.hide();
                            }
                        });
                    }),
                    save: Box::new(move |mem| {
                        if let Ok(s) = serde_json::to_string_pretty(mem) {
                            let _ = std::fs::write(&mem_path, s);
                        }
                    }),
                })
            })?;

            // ---- windows
            if let Some(main) = app.get_webview_window("main") {
                style_window(&main, true);
                let h = handle.clone();
                main.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        hide_flyout(&h);
                    }
                });
            }
            if let Some(osd) = app.get_webview_window("osd") {
                style_window(&osd, true);
                let _ = osd.set_ignore_cursor_events(true);
            }

            // ---- tray
            let open_i = MenuItem::with_id(app, "open", "Open Mixdeck", true, None::<&str>)?;
            let sound_i = MenuItem::with_id(app, "sound", "Windows sound settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Mixdeck", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &sound_i, &PredefinedMenuItem::separator(app)?, &quit_i])?;
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("Mixdeck: click to open, Ctrl+Alt+↑/↓ for the active app")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_flyout(app, None),
                    "sound" => open_sound_settings(),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = e {
                        toggle_flyout(tray.app_handle(), Some(position));
                    }
                })
                .build(app)?;

            // ---- hotkeys (ignore failures, e.g. another app already owns the combo)
            let ca = Modifiers::CONTROL | Modifiers::ALT;
            for code in [Code::ArrowUp, Code::ArrowDown, Code::KeyM] {
                if let Err(e) = app.global_shortcut().register(Shortcut::new(Some(ca), code)) {
                    eprintln!("hotkey {code:?} unavailable: {e}");
                }
            }

            if !start_minimized {
                show_flyout(&handle, None);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_mixer, set_volume, set_mute, set_master_volume, set_master_mute, select_device, set_remember, forget,
            fit_window, hide_window, get_autostart, set_autostart, open_sound_settings, quit,
            get_tabs, set_tab_volume, set_tab_mute, get_browser_status, open_extension_folder
        ])
        .build(tauri::generate_context!())
        .expect("error while building Mixdeck")
        .run(|_app, e| {
            // keep running in the tray when windows are closed
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = e {
                api.prevent_exit();
            }
        });
}
