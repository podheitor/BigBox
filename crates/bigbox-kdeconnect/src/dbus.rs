// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Linux backend: a D-Bus client to the **system** `kdeconnectd`. On KDE
//! desktops (BigLinux) the daemon already owns port 1716 and the phone's
//! pairing, so BigBox reuses it instead of running its own peer.
//!
//! Maps the `org.kde.kdeconnect.device.conversations` interface onto the shared
//! [`SmsBackend`] + [`Event`] model. KDE's D-Bus names are lowerCamelCase, so
//! every method/signal/property is given an explicit `name`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::sync::OnceCell;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{proxy, Connection};

use bigbox_core::sms::{SmsAddress, SmsMessage};

use crate::{Config, Event, PairedDevice, SmsBackend};

const DEVICE_PATH_PREFIX: &str = "/modules/kdeconnect/devices/";

#[proxy(
    interface = "org.kde.kdeconnect.daemon",
    default_service = "org.kde.kdeconnect",
    default_path = "/modules/kdeconnect"
)]
trait Daemon {
    #[zbus(name = "devices")]
    fn devices(&self, only_reachable: bool, only_paired: bool) -> zbus::Result<Vec<String>>;
    #[zbus(signal, name = "deviceAdded")]
    fn device_added(&self, id: String) -> zbus::Result<()>;
    #[zbus(signal, name = "deviceRemoved")]
    fn device_removed(&self, id: String) -> zbus::Result<()>;
    #[zbus(signal, name = "deviceVisibilityChanged")]
    fn device_visibility_changed(&self, id: String, is_visible: bool) -> zbus::Result<()>;
}

#[proxy(interface = "org.kde.kdeconnect.device", default_service = "org.kde.kdeconnect")]
trait Device {
    #[zbus(property, name = "name")]
    fn name(&self) -> zbus::Result<String>;
    #[zbus(property, name = "isReachable")]
    fn is_reachable(&self) -> zbus::Result<bool>;
    #[zbus(name = "requestPairing")]
    fn request_pairing(&self) -> zbus::Result<()>;
    #[zbus(name = "acceptPairing")]
    fn accept_pairing(&self) -> zbus::Result<()>;
    #[zbus(name = "unpair")]
    fn unpair(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.kde.kdeconnect.device.conversations",
    default_service = "org.kde.kdeconnect"
)]
trait Conversations {
    #[zbus(name = "requestAllConversationThreads")]
    fn request_all_conversation_threads(&self) -> zbus::Result<()>;
    #[zbus(name = "requestConversation")]
    fn request_conversation(&self, conversation_id: i64, start: i32, end: i32) -> zbus::Result<()>;
    #[zbus(name = "sendWithoutConversation")]
    fn send_without_conversation(
        &self,
        addresses: Vec<Value<'_>>,
        message: &str,
        attachment_urls: Vec<Value<'_>>,
    ) -> zbus::Result<()>;
    #[zbus(signal, name = "conversationCreated")]
    fn conversation_created(&self, msg: OwnedValue) -> zbus::Result<()>;
    #[zbus(signal, name = "conversationUpdated")]
    fn conversation_updated(&self, msg: OwnedValue) -> zbus::Result<()>;
    // Fired (instead of conversationCreated) when the daemon is already warm:
    // carries only the thread id + count, no message body. We react by pulling
    // the thread's latest message via requestConversation.
    #[zbus(signal, name = "conversationLoaded")]
    fn conversation_loaded(&self, conversation_id: i64, message_count: u64) -> zbus::Result<()>;
}

// ── Engine state ─────────────────────────────────────────────────
#[allow(dead_code)] // id/name kept for diagnostics; only `path` is used today
struct ActiveDevice {
    id: String,
    path: String,
    name: String,
}

#[derive(Default)]
struct State {
    devices: HashMap<String, PairedDevice>,
    active: Option<ActiveDevice>,
    /// Thread the pane currently has open — drives which Thread events we emit.
    active_open: Option<i64>,
    /// Accumulated messages per thread (deduped).
    threads: HashMap<i64, Vec<SmsMessage>>,
    /// Threads we have already pulled (or have in flight) via requestConversation
    /// in the warm-daemon path, to avoid duplicate requests.
    requested: std::collections::HashSet<i64>,
    /// Path we have already subscribed conversation signals on.
    subscribed: Option<String>,
}

struct Inner {
    events: mpsc::UnboundedSender<Event>,
    conn: OnceCell<Connection>,
    state: Mutex<State>,
    /// Tokio runtime handle, captured when the engine runs. Lets the
    /// fire-and-forget backend methods spawn D-Bus work even when called from a
    /// Tauri IPC thread that has no ambient tokio reactor.
    rt: OnceCell<tokio::runtime::Handle>,
    /// Queue of thread ids to fetch in the warm-daemon path. A single worker
    /// drains it one-at-a-time so we never flood `kdeconnectd` (each
    /// requestConversation triggers a cascade of conversationLoaded signals;
    /// firing hundreds at once wedges the daemon).
    loader: OnceCell<mpsc::UnboundedSender<i64>>,
    /// Wall-clock ms at engine start; SMS older than this are historical (no toast).
    started_ms: i64,
}

impl Inner {
    fn emit(&self, ev: Event) {
        let _ = self.events.send(ev);
    }
    fn active_path(&self) -> Option<String> {
        self.state.lock().unwrap().active.as_ref().map(|a| a.path.clone())
    }
    /// Spawn on the captured runtime (no-op until the engine has started).
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Some(rt) = self.rt.get() {
            rt.spawn(fut);
        }
    }
}

#[derive(Clone)]
pub struct DbusHandle {
    inner: Arc<Inner>,
}

pub struct DbusEngine {
    inner: Arc<Inner>,
}

/// Build the handle + engine synchronously; the D-Bus connection is established
/// in `DbusEngine::run` (async).
pub fn start(_cfg: Config, events: mpsc::UnboundedSender<Event>) -> (DbusHandle, DbusEngine) {
    let inner = Arc::new(Inner {
        events,
        conn: OnceCell::new(),
        state: Mutex::new(State::default()),
        rt: OnceCell::new(),
        loader: OnceCell::new(),
        started_ms: crate::packet::now_ms(),
    });
    (DbusHandle { inner: inner.clone() }, DbusEngine { inner })
}

fn device_path(device_id: &str) -> String {
    format!("{DEVICE_PATH_PREFIX}{device_id}")
}

// ── Backend (fire-and-forget) ────────────────────────────────────
impl SmsBackend for DbusHandle {
    fn devices(&self) -> Vec<PairedDevice> {
        self.inner.state.lock().unwrap().devices.values().cloned().collect()
    }

    fn list_conversations(&self) -> bool {
        // Emit whatever we already have cached, so a pane that opens *after* the
        // engine's startup sync still gets the full list (the warm-path dedup
        // won't re-emit those threads on a fresh request).
        {
            let st = self.inner.state.lock().unwrap();
            let rows: Vec<_> = st
                .threads
                .values()
                .filter_map(|msgs| crate::sms::conversation_from_messages(msgs))
                .collect();
            drop(st);
            if !rows.is_empty() {
                self.inner.emit(Event::Conversations(rows));
            }
        }
        // Also re-request from the phone for freshness.
        let Some(path) = self.inner.active_path() else { return true };
        let inner = self.inner.clone();
        self.inner.spawn(async move {
            if let Some(conn) = inner.conn.get() {
                if let Ok(p) = ConversationsProxy::builder(conn).path(path).unwrap().build().await {
                    let _ = p.request_all_conversation_threads().await;
                }
            }
        });
        true
    }

    fn load_thread(&self, thread_id: i64) -> bool {
        let Some(path) = self.inner.active_path() else { return false };
        // Mark open + emit whatever we already have cached.
        {
            let mut st = self.inner.state.lock().unwrap();
            st.active_open = Some(thread_id);
            if let Some(msgs) = st.threads.get(&thread_id) {
                let mut v = msgs.clone();
                v.sort_by_key(|m| m.date);
                self.inner.emit(Event::Thread { thread_id, messages: v });
            }
        }
        let inner = self.inner.clone();
        self.inner.spawn(async move {
            if let Some(conn) = inner.conn.get() {
                if let Ok(p) = ConversationsProxy::builder(conn).path(path).unwrap().build().await {
                    let _ = p.request_conversation(thread_id, 0, 50).await;
                }
            }
        });
        true
    }

    fn send_sms(&self, addresses: Vec<String>, body: String) -> bool {
        let Some(path) = self.inner.active_path() else { return false };
        let inner = self.inner.clone();
        self.inner.spawn(async move {
            if let Some(conn) = inner.conn.get() {
                if let Ok(p) = ConversationsProxy::builder(conn).path(path).unwrap().build().await {
                    let addrs: Vec<Value> =
                        addresses.iter().map(|a| Value::from(a.as_str())).collect();
                    let _ = p
                        .send_without_conversation(addrs, &body, Vec::new())
                        .await;
                }
            }
        });
        true
    }

    fn request_pair(&self, device_id: &str) -> bool {
        device_action(&self.inner, device_id, DeviceAction::Request);
        true
    }
    fn accept_pair(&self, device_id: &str) -> bool {
        device_action(&self.inner, device_id, DeviceAction::Accept);
        true
    }
    fn unpair(&self, device_id: &str) -> bool {
        device_action(&self.inner, device_id, DeviceAction::Unpair);
        true
    }
}

enum DeviceAction {
    Request,
    Accept,
    Unpair,
}

fn device_action(inner: &Arc<Inner>, device_id: &str, action: DeviceAction) {
    let path = device_path(device_id);
    let inner2 = inner.clone();
    inner.spawn(async move {
        if let Some(conn) = inner2.conn.get() {
            if let Ok(d) = DeviceProxy::builder(conn).path(path).unwrap().build().await {
                let _ = match action {
                    DeviceAction::Request => d.request_pairing().await,
                    DeviceAction::Accept => d.accept_pairing().await,
                    DeviceAction::Unpair => d.unpair().await,
                };
            }
        }
    });
}

// ── Engine driver ────────────────────────────────────────────────
impl DbusEngine {
    pub async fn run(self) {
        let inner = self.inner;
        // Capture the runtime so IPC-thread calls can spawn onto it.
        let _ = inner.rt.set(tokio::runtime::Handle::current());
        let conn = match Connection::session().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[bigbox] KDE Connect D-Bus session failed: {e}");
                return;
            }
        };
        let _ = inner.conn.set(conn.clone());

        // Single serialized loader for the warm-daemon path (throttled).
        {
            let (ltx, lrx) = mpsc::unbounded_channel::<i64>();
            let _ = inner.loader.set(ltx);
            tokio::spawn(loader_worker(inner.clone(), lrx));
        }

        let daemon = match DaemonProxy::new(&conn).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[bigbox] kdeconnect daemon proxy failed: {e}");
                return;
            }
        };

        refresh_devices(&inner, &conn, &daemon).await;
        subscribe_active(&inner, &conn).await;

        // React to device add/remove/visibility for the lifetime of the app.
        let added = daemon.receive_device_added().await.ok();
        let removed = daemon.receive_device_removed().await.ok();
        let vis = daemon.receive_device_visibility_changed().await.ok();

        if let Some(mut s) = added {
            let inner = inner.clone();
            let conn = conn.clone();
            tokio::spawn(async move {
                while s.next().await.is_some() {
                    let d = DaemonProxy::new(&conn).await;
                    if let Ok(d) = d {
                        refresh_devices(&inner, &conn, &d).await;
                    }
                    subscribe_active(&inner, &conn).await;
                }
            });
        }
        if let Some(mut s) = vis {
            let inner = inner.clone();
            let conn = conn.clone();
            tokio::spawn(async move {
                while s.next().await.is_some() {
                    if let Ok(d) = DaemonProxy::new(&conn).await {
                        refresh_devices(&inner, &conn, &d).await;
                    }
                    subscribe_active(&inner, &conn).await;
                }
            });
        }
        if let Some(mut s) = removed {
            let inner = inner.clone();
            let conn = conn.clone();
            tokio::spawn(async move {
                while s.next().await.is_some() {
                    if let Ok(d) = DaemonProxy::new(&conn).await {
                        refresh_devices(&inner, &conn, &d).await;
                    }
                }
            });
        }

        // Keep the engine task alive.
        std::future::pending::<()>().await;
    }
}

async fn refresh_devices(inner: &Arc<Inner>, conn: &Connection, daemon: &DaemonProxy<'_>) {
    let all = daemon.devices(false, false).await.unwrap_or_default();
    let paired: std::collections::HashSet<String> =
        daemon.devices(false, true).await.unwrap_or_default().into_iter().collect();

    let mut snapshot: Vec<PairedDevice> = Vec::new();
    let mut active: Option<ActiveDevice> = None;

    for id in all {
        let path = device_path(&id);
        let (name, reachable) =
            match DeviceProxy::builder(conn).path(path.clone()).unwrap().build().await {
                Ok(p) => (
                    p.name().await.unwrap_or_else(|_| id.clone()),
                    p.is_reachable().await.unwrap_or(false),
                ),
                Err(_) => (id.clone(), false),
            };
        let is_paired = paired.contains(&id);
        let pd = PairedDevice {
            device_id: id.clone(),
            name: name.clone(),
            paired: is_paired,
            reachable,
        };
        if is_paired && reachable && active.is_none() {
            active = Some(ActiveDevice { id: id.clone(), path, name });
        }
        snapshot.push(pd);
    }

    {
        let mut st = inner.state.lock().unwrap();
        st.devices = snapshot.iter().map(|d| (d.device_id.clone(), d.clone())).collect();
        st.active = active;
    }
    for d in snapshot {
        inner.emit(Event::DeviceUpdated(d));
    }
}

/// Subscribe conversation signals for the active device (once) and pull the
/// thread list.
async fn subscribe_active(inner: &Arc<Inner>, conn: &Connection) {
    let path = {
        let st = inner.state.lock().unwrap();
        match &st.active {
            Some(a) => {
                if st.subscribed.as_deref() == Some(a.path.as_str()) {
                    return;
                }
                a.path.clone()
            }
            None => return,
        }
    };
    inner.state.lock().unwrap().subscribed = Some(path.clone());

    let proxy = match ConversationsProxy::builder(conn).path(path).unwrap().build().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[bigbox] conversations proxy failed: {e}");
            return;
        }
    };

    if let Ok(mut created) = proxy.receive_conversation_created().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            while let Some(sig) = created.next().await {
                if let Ok(args) = sig.args() {
                    handle_msg(&inner, args.msg);
                }
            }
        });
    }
    if let Ok(mut updated) = proxy.receive_conversation_updated().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            while let Some(sig) = updated.next().await {
                if let Ok(args) = sig.args() {
                    handle_msg(&inner, args.msg);
                }
            }
        });
    }
    // Warm-daemon path: conversationLoaded gives only the thread id; pull the
    // thread's latest message so the conversation list still populates.
    if let Ok(mut loaded) = proxy.receive_conversation_loaded().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            while let Some(sig) = loaded.next().await {
                if let Ok(args) = sig.args() {
                    on_conversation_loaded(&inner, args.conversation_id);
                }
            }
        });
    }

    let _ = proxy.request_all_conversation_threads().await;
}

/// React to a `conversationLoaded` (warm daemon): if we don't already have the
/// thread, pull its latest message via `requestConversation` — it arrives as a
/// `conversationUpdated` and flows through [`handle_msg`].
fn on_conversation_loaded(inner: &Arc<Inner>, thread_id: i64) {
    {
        let mut st = inner.state.lock().unwrap();
        if st.threads.contains_key(&thread_id) || st.requested.contains(&thread_id) {
            return;
        }
        st.requested.insert(thread_id);
    }
    // Hand off to the serialized loader instead of firing immediately.
    if let Some(tx) = inner.loader.get() {
        let _ = tx.send(thread_id);
    }
}

/// Drains queued thread ids one at a time, pulling each thread's latest message
/// with a small gap so the daemon is never flooded.
async fn loader_worker(inner: Arc<Inner>, mut rx: mpsc::UnboundedReceiver<i64>) {
    while let Some(thread_id) = rx.recv().await {
        let Some(path) = inner.active_path() else { continue };
        if let Some(conn) = inner.conn.get() {
            if let Ok(p) = ConversationsProxy::builder(conn).path(path).unwrap().build().await {
                let _ = p.request_conversation(thread_id, 0, 1).await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    }
}

/// Ingest one message variant: cache it, refresh the list row, update an open
/// thread, and toast if it's a genuinely-new inbound message.
fn handle_msg(inner: &Arc<Inner>, owned: OwnedValue) {
    let Some(m) = parse_message(&owned) else { return };

    let (row, thread_snapshot, notify) = {
        let mut st = inner.state.lock().unwrap();

        let entry = st.threads.entry(m.thread_id).or_default();
        let dup = entry
            .iter()
            .any(|x| x.date == m.date && x.from_me == m.from_me && x.body == m.body);
        if !dup {
            entry.push(m.clone());
        }
        let row = crate::sms::conversation_from_messages(entry);

        let thread_snapshot = if st.active_open == Some(m.thread_id) {
            let mut v = st.threads.get(&m.thread_id).cloned().unwrap_or_default();
            v.sort_by_key(|x| x.date);
            Some(v)
        } else {
            None
        };

        // Notify only for genuinely-new inbound SMS: ones that arrived after
        // BigBox started. Historical messages (the initial bulk load) have a
        // date before start, so they never toast.
        let notify = !m.from_me && !dup && m.date > inner.started_ms;
        (row, thread_snapshot, notify)
    };

    if let Some(r) = row {
        inner.emit(Event::Conversations(vec![r]));
    }
    if let Some(messages) = thread_snapshot {
        inner.emit(Event::Thread { thread_id: m.thread_id, messages });
    }
    if notify {
        inner.emit(Event::Incoming(m));
    }
}

// ── Variant parsing ──────────────────────────────────────────────
// KDE Connect's ConversationMessage crosses D-Bus as a STRUCT, not a dict:
//   (i s a(s) x i i x i x a(xsss))
//   = (event, body, addresses, date, type, read, threadID, uID, subID,
//      attachments). Parsed positionally below. type 2 == sent by the owner.
fn parse_message(owned: &OwnedValue) -> Option<SmsMessage> {
    let v: &Value = owned;
    let fields = match unwrap_variant(v) {
        Value::Structure(s) => s.fields(),
        _ => return None,
    };
    if fields.len() < 7 {
        return None;
    }
    let body = value_str(&fields[1]).unwrap_or_default();
    let addresses = parse_addresses(&fields[2]);
    let date = value_i64(&fields[3]).unwrap_or(0);
    let mtype = value_i64(&fields[4]).unwrap_or(1);
    let read = value_i64(&fields[5]).unwrap_or(1) != 0;
    let thread_id = value_i64(&fields[6])?;
    let attachment_count = fields
        .get(9)
        .and_then(|f| match unwrap_variant(f) {
            Value::Array(a) => Some(a.len() as u32),
            _ => None,
        })
        .unwrap_or(0);

    Some(SmsMessage {
        thread_id,
        from_me: mtype == 2,
        body,
        date,
        read,
        addresses,
        attachment_count,
    })
}

/// `addresses` is `a(s)` — an array of single-string structs.
fn parse_addresses(v: &Value) -> Vec<SmsAddress> {
    let arr = match unwrap_variant(v) {
        Value::Array(a) => a,
        _ => return Vec::new(),
    };
    arr.iter()
        .filter_map(|item| {
            let address = match unwrap_variant(item) {
                Value::Structure(s) => s.fields().first().and_then(value_str),
                other => value_str(other),
            }?;
            Some(SmsAddress { address, display_name: None })
        })
        .collect()
}

fn unwrap_variant<'a>(v: &'a Value<'a>) -> &'a Value<'a> {
    match v {
        Value::Value(inner) => unwrap_variant(inner),
        other => other,
    }
}

fn value_i64(v: &Value) -> Option<i64> {
    match unwrap_variant(v) {
        Value::I64(n) => Some(*n),
        Value::U64(n) => Some(*n as i64),
        Value::I32(n) => Some(*n as i64),
        Value::U32(n) => Some(*n as i64),
        Value::I16(n) => Some(*n as i64),
        Value::U16(n) => Some(*n as i64),
        Value::U8(n) => Some(*n as i64),
        Value::Str(s) => s.as_str().parse().ok(),
        _ => None,
    }
}

fn value_str(v: &Value) -> Option<String> {
    match unwrap_variant(v) {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

