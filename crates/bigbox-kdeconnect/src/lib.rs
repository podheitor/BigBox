// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! KDE Connect SMS backend for BigBox, split per-OS behind one
//! [`KdeConnectHandle`] + [`Event`] stream so the shell and frontend never see
//! the difference:
//!
//! - **Linux** ([`dbus`]): BigLinux/KDE already runs the system `kdeconnectd`,
//!   which owns port 1716 and the phone's pairing. BigBox talks to it over
//!   D-Bus (`org.kde.kdeconnect.device.conversations`) and reuses that pairing.
//! - **Windows/other** (native peer: [`connection`]/[`discovery`]/[`tls`]):
//!   no system KDE Connect, so BigBox *is* the paired device — LAN discovery +
//!   mutual TLS + trust-on-first-use pairing, implemented from scratch.
//!
//! The shell calls [`new`] once, manages the [`KdeConnectHandle`] for the IPC
//! commands, spawns [`Engine::run`], and forwards the [`Event`] stream.

pub mod identity;
pub mod packet;
pub mod sms;

#[cfg(target_os = "linux")]
mod dbus;

#[cfg(not(target_os = "linux"))]
pub mod cert;
#[cfg(not(target_os = "linux"))]
pub mod connection;
#[cfg(not(target_os = "linux"))]
pub mod discovery;
#[cfg(not(target_os = "linux"))]
pub mod store;
#[cfg(not(target_os = "linux"))]
pub mod tls;

pub use bigbox_core::sms::{Conversation, PairedDevice, SmsAddress, SmsMessage};

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

/// KDE Connect uses ports 1716-1764 (native peer only).
pub const DISCOVERY_PORT: u16 = 1716;
pub const PORT_MIN: u16 = 1716;
pub const PORT_MAX: u16 = 1764;

/// Engine configuration supplied by the shell.
#[derive(Clone)]
pub struct Config {
    /// Stable device id; only used by the native peer to seed its cert.
    pub device_id: String,
    /// Human-facing name the phone shows for this BigBox (native peer only).
    pub device_name: String,
    /// Base config dir; the native peer stores cert + trust under it.
    pub config_dir: PathBuf,
}

/// Things a backend surfaces to the shell, forwarded to the frontend.
#[derive(Debug, Clone)]
pub enum Event {
    /// Conversation-list rows (latest message per thread).
    Conversations(Vec<Conversation>),
    /// All messages of one thread.
    Thread {
        thread_id: i64,
        messages: Vec<SmsMessage>,
    },
    /// A freshly-arrived inbound message (drives the desktop toast).
    Incoming(SmsMessage),
    /// Current number of conversations with unread inbound messages (the
    /// sidebar/tray badge count).
    Unread(u32),
    /// A device's discovery / pairing / reachability state changed.
    DeviceUpdated(PairedDevice),
    /// The phone asked to pair; the user must accept in the pane.
    PairingRequest { device_id: String, name: String },
}

/// What both backends implement; `KdeConnectHandle` is a thin facade over it.
/// Methods are fire-and-forget — results return asynchronously as [`Event`]s.
pub(crate) trait SmsBackend: Send + Sync {
    fn devices(&self) -> Vec<PairedDevice>;
    fn list_conversations(&self) -> bool;
    fn load_thread(&self, thread_id: i64) -> bool;
    fn send_sms(&self, addresses: Vec<String>, body: String) -> bool;
    fn request_pair(&self, device_id: &str) -> bool;
    fn accept_pair(&self, device_id: &str) -> bool;
    fn unpair(&self, device_id: &str) -> bool;
}

/// Public, cloneable control surface the shell IPC commands call.
#[derive(Clone)]
pub struct KdeConnectHandle {
    inner: Arc<dyn SmsBackend>,
}

impl KdeConnectHandle {
    pub fn devices(&self) -> Vec<PairedDevice> {
        self.inner.devices()
    }
    pub fn list_conversations(&self) -> bool {
        self.inner.list_conversations()
    }
    pub fn load_thread(&self, thread_id: i64) -> bool {
        self.inner.load_thread(thread_id)
    }
    pub fn send_sms(&self, addresses: Vec<String>, body: String) -> bool {
        self.inner.send_sms(addresses, body)
    }
    pub fn request_pair(&self, device_id: &str) -> bool {
        self.inner.request_pair(device_id)
    }
    pub fn accept_pair(&self, device_id: &str) -> bool {
        self.inner.accept_pair(device_id)
    }
    pub fn unpair(&self, device_id: &str) -> bool {
        self.inner.unpair(device_id)
    }
}

/// The driver that owns the background tasks; the shell spawns `run()` once.
pub struct Engine {
    driver: EngineDriver,
}

#[cfg(target_os = "linux")]
type EngineDriver = dbus::DbusEngine;
#[cfg(not(target_os = "linux"))]
type EngineDriver = native::NativeEngine;

impl Engine {
    pub async fn run(self) {
        self.driver.run().await
    }
}

/// Build the backend synchronously (so the shell can `manage` the handle before
/// any IPC fires). The returned [`Engine`] must be `run()` inside a tokio
/// runtime (the shell spawns it on tauri's runtime).
#[cfg(target_os = "linux")]
pub fn new(cfg: Config) -> io::Result<(KdeConnectHandle, mpsc::UnboundedReceiver<Event>, Engine)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let (handle, engine) = dbus::start(cfg, tx);
    Ok((
        KdeConnectHandle {
            inner: Arc::new(handle),
        },
        rx,
        Engine { driver: engine },
    ))
}

#[cfg(not(target_os = "linux"))]
pub fn new(cfg: Config) -> io::Result<(KdeConnectHandle, mpsc::UnboundedReceiver<Event>, Engine)> {
    let (inner, rx) = native::build(cfg)?;
    Ok((
        KdeConnectHandle {
            inner: inner.clone(),
        },
        rx,
        Engine {
            driver: native::NativeEngine { inner },
        },
    ))
}

// ── Native peer (Windows/other) ──────────────────────────────────
#[cfg(not(target_os = "linux"))]
mod native {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::AtomicU16;
    use std::sync::Mutex;
    use tokio_rustls::rustls::{ClientConfig, ServerConfig};

    use crate::cert::DeviceCert;
    use crate::identity::Identity;
    use crate::packet::NetworkPacket;
    use crate::store::{self, TrustStore};

    pub(crate) struct Link {
        pub name: String,
        pub tx: mpsc::UnboundedSender<NetworkPacket>,
        pub fingerprint: String,
        pub paired: bool,
    }

    pub struct EngineInner {
        pub(crate) cfg: Config,
        pub(crate) cert: DeviceCert,
        pub(crate) server_cfg: Arc<ServerConfig>,
        pub(crate) client_cfg: Arc<ClientConfig>,
        pub(crate) trust: Mutex<TrustStore>,
        pub(crate) links: Mutex<HashMap<String, Link>>,
        pub(crate) pending_pair: Mutex<HashSet<String>>,
        pub(crate) connecting: Mutex<HashSet<String>>,
        pub(crate) tcp_port: AtomicU16,
        pub(crate) events: mpsc::UnboundedSender<Event>,
    }

    impl EngineInner {
        pub(crate) fn device_id(&self) -> &str {
            &self.cert.device_id
        }
        pub(crate) fn identity(&self, tcp_port: Option<u16>) -> Identity {
            Identity::this_device(&self.cert.device_id, &self.cfg.device_name, tcp_port)
        }
        pub(crate) fn emit(&self, ev: Event) {
            let _ = self.events.send(ev);
        }
        pub(crate) fn send_to(&self, device_id: &str, pkt: NetworkPacket) -> bool {
            let links = self.links.lock().unwrap();
            match links.get(device_id) {
                Some(link) => link.tx.send(pkt).is_ok(),
                None => false,
            }
        }
        pub(crate) fn active_device(&self) -> Option<String> {
            let links = self.links.lock().unwrap();
            links.iter().find(|(_, l)| l.paired).map(|(id, _)| id.clone())
        }
        pub(crate) fn snapshot_devices(&self) -> Vec<PairedDevice> {
            let links = self.links.lock().unwrap();
            let trust = self.trust.lock().unwrap();
            let mut out: HashMap<String, PairedDevice> = HashMap::new();
            for d in trust.list() {
                out.insert(
                    d.device_id.clone(),
                    PairedDevice {
                        device_id: d.device_id,
                        name: d.name,
                        paired: true,
                        reachable: false,
                    },
                );
            }
            for (id, link) in links.iter() {
                let entry = out.entry(id.clone()).or_insert_with(|| PairedDevice {
                    device_id: id.clone(),
                    name: link.name.clone(),
                    paired: link.paired,
                    reachable: false,
                });
                entry.reachable = true;
                entry.paired = link.paired;
                entry.name = link.name.clone();
            }
            out.into_values().collect()
        }
    }

    impl SmsBackend for EngineInner {
        fn devices(&self) -> Vec<PairedDevice> {
            self.snapshot_devices()
        }
        fn list_conversations(&self) -> bool {
            match self.active_device() {
                Some(id) => self.send_to(&id, crate::sms::request_conversations()),
                None => false,
            }
        }
        fn load_thread(&self, thread_id: i64) -> bool {
            match self.active_device() {
                Some(id) => self.send_to(&id, crate::sms::request_conversation(thread_id)),
                None => false,
            }
        }
        fn send_sms(&self, addresses: Vec<String>, body: String) -> bool {
            match self.active_device() {
                Some(id) => self.send_to(&id, crate::sms::send_sms(&addresses, &body)),
                None => false,
            }
        }
        fn request_pair(&self, device_id: &str) -> bool {
            self.pending_pair.lock().unwrap().insert(device_id.to_string());
            self.send_to(device_id, crate::connection::pair_packet(true))
        }
        fn accept_pair(&self, device_id: &str) -> bool {
            crate::connection::complete_pairing(self, device_id);
            self.send_to(device_id, crate::connection::pair_packet(true))
        }
        fn unpair(&self, device_id: &str) -> bool {
            let _ = self.trust.lock().unwrap().remove(device_id);
            if let Some(link) = self.links.lock().unwrap().get_mut(device_id) {
                link.paired = false;
            }
            self.send_to(device_id, crate::connection::pair_packet(false))
        }
    }

    pub struct NativeEngine {
        pub(crate) inner: Arc<EngineInner>,
    }

    impl NativeEngine {
        pub async fn run(self) {
            let recv = tokio::spawn(crate::discovery::recv_loop(self.inner.clone()));
            let bcast = tokio::spawn(crate::discovery::broadcast_loop(self.inner.clone()));
            let listen = tokio::spawn(crate::connection::tcp_listener(self.inner.clone()));
            let _ = tokio::join!(recv, bcast, listen);
        }
    }

    pub fn build(cfg: Config) -> io::Result<(Arc<EngineInner>, mpsc::UnboundedReceiver<Event>)> {
        let cert_dir = cfg.config_dir.join("kdeconnect");
        let cert = DeviceCert::load_or_create(&cert_dir, &cfg.device_id)?;
        let trust = TrustStore::load(store::store_path(&cert_dir));
        let server_cfg = crate::tls::server_config(&cert)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let client_cfg = crate::tls::client_config(&cert)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let (tx, rx) = mpsc::unbounded_channel();
        let mut cfg = cfg;
        cfg.device_id = cert.device_id.clone();

        let inner = Arc::new(EngineInner {
            cfg,
            cert,
            server_cfg,
            client_cfg,
            trust: Mutex::new(trust),
            links: Mutex::new(HashMap::new()),
            pending_pair: Mutex::new(HashSet::new()),
            connecting: Mutex::new(HashSet::new()),
            tcp_port: AtomicU16::new(0),
            events: tx,
        });
        Ok((inner, rx))
    }
}

#[cfg(not(target_os = "linux"))]
pub use native::EngineInner;
#[cfg(not(target_os = "linux"))]
pub(crate) use native::Link;
