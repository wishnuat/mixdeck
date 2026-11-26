// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Chrome/Edge start us as the native-messaging host with the extension origin as the first argument:
    // relay messages instead of starting the UI (and before the single-instance check).
    if let Some(origin) = mixdeck_lib::tabs::is_bridge_launch() {
        mixdeck_lib::tabs::run_bridge(&origin);
    }
    mixdeck_lib::run()
}
