// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! TCP/TLS link establishment, the per-link read/write loop, and packet
//! dispatch (pairing + SMS).
//!
//! KDE Connect role rule: the side that *initiates* the TCP connection sends its
//! identity first and becomes the **TLS server**; the side that *accepts*
//! becomes the **TLS client**. BigBox does both — it dials phones it hears via
//! UDP (→ TLS server) and accepts phones that dial it (→ TLS client).
//! VALIDATE: confirm this direction against a live Android client.

use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::cert;
use crate::identity::{Identity, PACKET_TYPE_IDENTITY};
use crate::packet::NetworkPacket;
use crate::sms;
use crate::{EngineInner, Event, Link, PairedDevice, PORT_MAX, PORT_MIN};

const PAIR_TYPE: &str = "kdeconnect.pair";

/// Build a `kdeconnect.pair` packet.
pub fn pair_packet(pair: bool) -> NetworkPacket {
    NetworkPacket::new(PAIR_TYPE, json!({ "pair": pair }))
}

/// Pin a device's pinned cert fingerprint and mark its link paired.
pub(crate) fn complete_pairing(inner: &EngineInner, device_id: &str) {
    let (name, fp) = {
        let links = inner.links.lock().unwrap();
        match links.get(device_id) {
            Some(l) => (l.name.clone(), l.fingerprint.clone()),
            None => return,
        }
    };
    let _ = inner.trust.lock().unwrap().pin(device_id, &name, &fp);
    if let Some(l) = inner.links.lock().unwrap().get_mut(device_id) {
        l.paired = true;
    }
    inner.pending_pair.lock().unwrap().remove(device_id);
    inner.emit(Event::DeviceUpdated(PairedDevice {
        device_id: device_id.to_string(),
        name,
        paired: true,
        reachable: true,
    }));
}

/// Bind the TCP listener (first free port in the KDE Connect range) and accept
/// inbound links forever.
pub async fn tcp_listener(inner: Arc<EngineInner>) {
    let mut listener = None;
    for port in PORT_MIN..=PORT_MAX {
        if let Ok(l) = TcpListener::bind(("0.0.0.0", port)).await {
            inner.tcp_port.store(port, Ordering::Relaxed);
            listener = Some(l);
            break;
        }
    }
    let listener = match listener {
        Some(l) => l,
        None => return,
    };

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let inner = inner.clone();
                tokio::spawn(async move {
                    let _ = accept_link(inner, stream, addr.ip()).await;
                });
            }
            Err(_) => continue,
        }
    }
}

/// We accepted an inbound TCP connection: read the peer's plaintext identity,
/// then upgrade as the TLS **client**.
async fn accept_link(
    inner: Arc<EngineInner>,
    mut stream: TcpStream,
    _peer: IpAddr,
) -> std::io::Result<()> {
    let line = read_line_raw(&mut stream).await?;
    let ident = match parse_identity(&line) {
        Some(i) if i.device_id != inner.device_id() => i,
        _ => return Ok(()),
    };

    let server_name = ServerName::try_from("kdeconnect")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let connector = TlsConnector::from(inner.client_cfg.clone());
    let tls = connector.connect(server_name, stream).await?;
    let fp = {
        let (_io, conn) = tls.get_ref();
        peer_fingerprint(conn.peer_certificates())
    };

    run_link(inner, ident, fp, tls).await;
    Ok(())
}

/// We initiated a TCP connection (heard the phone via UDP): send our identity,
/// then upgrade as the TLS **server**.
pub(crate) async fn dial(
    inner: Arc<EngineInner>,
    ip: IpAddr,
    port: u16,
    ident: Identity,
) -> std::io::Result<()> {
    let mut stream = TcpStream::connect((ip, port)).await?;
    let line = inner.identity(None).to_packet().to_line().map_err(to_io)?;
    stream.write_all(line.as_bytes()).await?;

    let acceptor = TlsAcceptor::from(inner.server_cfg.clone());
    let tls = acceptor.accept(stream).await?;
    let fp = {
        let (_io, conn) = tls.get_ref();
        peer_fingerprint(conn.peer_certificates())
    };

    run_link(inner, ident, fp, tls).await;
    Ok(())
}

/// Register the link, pump packets both ways, and dispatch until the peer
/// disconnects.
async fn run_link<S>(inner: Arc<EngineInner>, ident: Identity, fingerprint: String, stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let device_id = ident.device_id.clone();
    let name = ident.device_name.clone();
    let paired = inner.trust.lock().unwrap().is_paired(&device_id);

    let (rd, mut wr) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::unbounded_channel::<NetworkPacket>();

    inner.links.lock().unwrap().insert(
        device_id.clone(),
        Link {
            name: name.clone(),
            tx: tx.clone(),
            fingerprint,
            paired,
        },
    );
    inner.connecting.lock().unwrap().remove(&device_id);
    inner.emit(Event::DeviceUpdated(PairedDevice {
        device_id: device_id.clone(),
        name: name.clone(),
        paired,
        reachable: true,
    }));

    let writer = tokio::spawn(async move {
        while let Some(pkt) = rx.recv().await {
            match pkt.to_line() {
                Ok(line) => {
                    if wr.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
    });

    // Once paired, prime the conversation list so badges/notifications work
    // even before the user opens the pane.
    if paired {
        let _ = tx.send(sms::request_conversations());
    }

    let mut lines = BufReader::new(rd).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(pkt) = NetworkPacket::from_line(&line) {
            handle_packet(&inner, &device_id, pkt);
        }
    }

    writer.abort();
    inner.links.lock().unwrap().remove(&device_id);
    inner.connecting.lock().unwrap().remove(&device_id);
    let still_paired = inner.trust.lock().unwrap().is_paired(&device_id);
    inner.emit(Event::DeviceUpdated(PairedDevice {
        device_id,
        name,
        paired: still_paired,
        reachable: false,
    }));
}

fn handle_packet(inner: &Arc<EngineInner>, device_id: &str, pkt: NetworkPacket) {
    match pkt.packet_type.as_str() {
        PAIR_TYPE => {
            let pair = pkt.body.get("pair").and_then(|v| v.as_bool()).unwrap_or(false);
            if pair {
                let awaited = inner.pending_pair.lock().unwrap().contains(device_id);
                if awaited {
                    // Confirmation of a pair request we initiated.
                    complete_pairing(inner, device_id);
                } else {
                    // The phone is asking us to pair; surface for user accept.
                    let name = inner
                        .links
                        .lock()
                        .unwrap()
                        .get(device_id)
                        .map(|l| l.name.clone())
                        .unwrap_or_default();
                    inner.emit(Event::PairingRequest {
                        device_id: device_id.to_string(),
                        name,
                    });
                }
            } else {
                // Unpair.
                let _ = inner.trust.lock().unwrap().remove(device_id);
                if let Some(l) = inner.links.lock().unwrap().get_mut(device_id) {
                    l.paired = false;
                }
                let name = inner
                    .links
                    .lock()
                    .unwrap()
                    .get(device_id)
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                inner.emit(Event::DeviceUpdated(PairedDevice {
                    device_id: device_id.to_string(),
                    name,
                    paired: false,
                    reachable: true,
                }));
            }
        }
        sms::TYPE_MESSAGES => {
            // Ignore SMS from devices we have not paired/pinned.
            if !inner.trust.lock().unwrap().is_paired(device_id) {
                return;
            }
            let messages = sms::parse_messages(&pkt.body);
            if messages.is_empty() {
                return;
            }
            let distinct: std::collections::HashSet<i64> =
                messages.iter().map(|m| m.thread_id).collect();

            if messages.len() == 1 {
                let m = messages[0].clone();
                if let Some(conv) = sms::conversation_from_messages(&messages) {
                    inner.emit(Event::Conversations(vec![conv]));
                }
                inner.emit(Event::Thread {
                    thread_id: m.thread_id,
                    messages: messages.clone(),
                });
                if !m.from_me {
                    inner.emit(Event::Incoming(m));
                }
            } else if distinct.len() == 1 {
                let thread_id = messages[0].thread_id;
                inner.emit(Event::Thread { thread_id, messages });
            } else {
                let mut by_thread: std::collections::HashMap<i64, Vec<crate::SmsMessage>> =
                    std::collections::HashMap::new();
                for m in messages {
                    by_thread.entry(m.thread_id).or_default().push(m);
                }
                let convs = by_thread
                    .values()
                    .filter_map(|msgs| sms::conversation_from_messages(msgs))
                    .collect();
                inner.emit(Event::Conversations(convs));
            }
        }
        _ => {}
    }
}

/// Read one newline-terminated line directly from the socket (byte-at-a-time so
/// no TLS bytes are consumed before the handshake). Caps at 64 KiB.
async fn read_line_raw(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        buf.push(byte[0]);
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn parse_identity(line: &str) -> Option<Identity> {
    let pkt = NetworkPacket::from_line(line.trim()).ok()?;
    if pkt.packet_type != PACKET_TYPE_IDENTITY {
        return None;
    }
    serde_json::from_value(pkt.body).ok()
}

fn peer_fingerprint(
    certs: Option<&[tokio_rustls::rustls::pki_types::CertificateDer<'_>]>,
) -> String {
    certs
        .and_then(|c| c.first())
        .map(|c| cert::fingerprint(c.as_ref()))
        .unwrap_or_default()
}

fn to_io(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}
