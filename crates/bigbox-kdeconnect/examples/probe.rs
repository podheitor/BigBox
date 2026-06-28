// Live probe: run the KDE Connect backend for a few seconds and print the
// events it emits. Validates the D-Bus integration against the real daemon.
// Run: cargo run -p bigbox-kdeconnect --example probe

use bigbox_kdeconnect::{new, Config, Event};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let cfg = Config {
        device_id: "bigbox-probe".into(),
        device_name: "BigBox Probe".into(),
        config_dir: std::env::temp_dir().join("bigbox-probe"),
    };
    let (handle, mut rx, engine) = new(cfg).expect("engine init");
    tokio::spawn(engine.run());

    tokio::time::sleep(Duration::from_millis(1500)).await;
    println!("== devices known after 1.5s ==");
    for d in handle.devices() {
        println!("  {d:?}");
    }
    println!("== requesting conversations ==");
    println!("  list_conversations() -> {}", handle.list_conversations());

    let mut threads = std::collections::HashSet::new();
    let mut incoming = 0;
    let mut unread = 0u32;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        tokio::select! {
            Some(ev) = rx.recv() => match ev {
                // Count unique threads — don't print message content (privacy).
                Event::Conversations(c) => for conv in c { threads.insert(conv.thread_id); },
                Event::Thread { thread_id, .. } => { threads.insert(thread_id); }
                Event::Incoming(_) => incoming += 1,
                Event::Unread(n) => unread = n,
                Event::DeviceUpdated(d) => println!("  DeviceUpdated: {} (paired={}, reachable={})",
                    d.name, d.paired, d.reachable),
                Event::PairingRequest { name, .. } => println!("  PairingRequest from {name}"),
            },
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    println!("== RESULT: {} conversations parsed, {} incoming, {} unread ==",
        threads.len(), incoming, unread);
}
