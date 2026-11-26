//! End-to-end: launch the real `mixdeck` binary the way Chrome does (native-messaging host with the extension
//! origin as argv[1]) and check that it relays tabs to the hub and commands back to the "extension".

use mixdeck_lib::tabs::{read_msg, start_hub, write_msg, EXTENSION_ID};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn bridge_relays_both_ways() {
    let appdata = std::env::temp_dir().join(format!("mixdeck-e2e-{}", std::process::id()));
    let dir = appdata.join("id.wishnu.mixdeck");
    let hub = start_hub(&dir, |_| {}).expect("hub");

    let mut child = Command::new(env!("CARGO_BIN_EXE_mixdeck"))
        .arg(format!("chrome-extension://{EXTENSION_ID}/"))
        .env("APPDATA", &appdata) // the bridge finds bridge.json under %APPDATA%
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn bridge");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();

    // extension → app
    write_msg(&mut stdin, br#"{"type":"tabs","tabs":[{"id":42,"title":"Lo-fi beats","host":"youtube.com","audible":true,"muted":false,"volume":0.6}]}"#).unwrap();
    stdin.flush().unwrap();

    let t0 = Instant::now();
    let snap = loop {
        let s = hub.snapshot();
        if s.first().is_some_and(|b| !b.tabs.is_empty()) {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "tabs never reached the hub: {s:?}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(snap[0].tabs[0].title, "Lo-fi beats");
    assert!(snap[0].browser.ends_with(".exe"), "browser key comes from the parent process: {}", snap[0].browser);

    // app → extension (the bridge first asks the extension for a fresh list)
    assert!(hub.send(snap[0].conn, &serde_json::json!({"type":"setVolume","tabId":42,"volume":0.25})));
    let mut got = Vec::new();
    while got.len() < 2 {
        let msg = read_msg(&mut stdout).unwrap().expect("bridge closed stdout");
        got.push(String::from_utf8(msg).unwrap());
    }
    assert_eq!(got[0], r#"{"type":"refresh"}"#);
    assert!(got[1].contains(r#""type":"setVolume""#) && got[1].contains(r#""tabId":42"#), "{got:?}");

    // browser closes the port → bridge exits
    drop(stdin);
    let t0 = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "bridge did not exit after stdin closed");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(appdata);
}
