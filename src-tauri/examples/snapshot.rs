//! Print what the engine sees: devices, master, apps and one round of peak levels.
//!   cargo run --example snapshot

use mixdeck_lib::audio::{run, Cmd, Hooks, Memory};
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    let (tx, rx) = mpsc::channel();
    let t = std::thread::spawn(move || {
        run(
            rx,
            Memory { remember: false, ..Default::default() },
            Hooks {
                mixer: Box::new(|m| {
                    println!("device: {}  (master {:.0}% muted={})", m.devices.iter().find(|d| d.id == m.device_id).map_or("?", |d| &d.name), m.master_volume * 100.0, m.master_muted);
                    for d in &m.devices {
                        println!("  out: {}{}", d.name, if d.is_default { "  [default]" } else { "" });
                    }
                    for a in &m.apps {
                        println!(
                            "  app: {:<28} {:>4.0}% muted={:<5} active={:<5} sessions={} icon={}",
                            a.name, a.volume * 100.0, a.muted, a.active, a.sessions,
                            a.icon.as_ref().map_or("-".to_string(), |i| format!("{} B", i.len()))
                        );
                    }
                    if let Some(e) = &m.error {
                        println!("  error: {e}");
                    }
                }),
                levels: Box::new(|l| {
                    let mut v: Vec<_> = l.apps.iter().filter(|(_, p)| **p > 0.001).collect();
                    v.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
                    println!("levels: master {:.3} {:?}", l.master, v);
                }),
                osd: Box::new(|_| {}),
                save: Box::new(|_| {}),
            },
        )
    });
    std::thread::sleep(Duration::from_millis(300));
    // MIXDECK_IDLE_SECS=n: just idle (metering off) for n seconds, to measure the engine's background cost
    if let Some(secs) = std::env::var("MIXDECK_IDLE_SECS").ok().and_then(|s| s.parse::<u64>().ok()) {
        std::thread::sleep(Duration::from_secs(secs));
    }
    tx.send(Cmd::SetMetering(true)).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    tx.send(Cmd::SetMetering(false)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    drop(tx);
    t.join().unwrap();
}
