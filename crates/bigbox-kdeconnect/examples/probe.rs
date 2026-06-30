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
    // Contacts: confirm the folder loads (KDE Connect cache + ~/.config/bigbox/contacts).
    let t = std::time::Instant::now();
    let contacts = handle.contacts();
    let with_photo = contacts.iter().filter(|c| c.photo.is_some()).count();
    println!("== CONTACTS: {} loaded ({with_photo} with photo) in {:?} ==",
        contacts.len(), t.elapsed());

    // Build the same number->name map the frontend builds, to measure how many
    // real conversations actually resolve to a contact name.
    let norm = |s: &str| -> String {
        let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
        if d.len() > 9 { d[d.len() - 9..].to_string() } else { d }
    };
    let mut by_number = std::collections::HashMap::<String, String>::new();
    for c in &contacts {
        for n in &c.numbers {
            let k = norm(n);
            if !k.is_empty() { by_number.entry(k).or_insert_with(|| c.name.clone()); }
        }
    }

    println!("== requesting conversations ==");
    println!("  list_conversations() -> {}", handle.list_conversations());

    let mut threads = std::collections::HashSet::new();
    let mut resolved = 0usize;
    let mut total_convs = 0usize;
    let mut samples: Vec<String> = Vec::new();
    let mut incoming = 0;
    let mut unread = 0u32;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        tokio::select! {
            Some(ev) = rx.recv() => match ev {
                Event::Conversations(c) => for conv in c {
                    if threads.insert(conv.thread_id) {
                        total_convs += 1;
                        let hit = conv.addresses.iter()
                            .find_map(|a| by_number.get(&norm(&a.address)));
                        if let Some(name) = hit {
                            resolved += 1;
                            if samples.len() < 8 { samples.push(name.clone()); }
                        }
                    }
                },
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
    println!("== RESULT: {} conversations, {} resolved to a contact name, {} incoming, {} unread ==",
        total_convs, resolved, incoming, unread);
    println!("== sample resolved names: {:?} ==", samples);
}
