// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! LAN discovery over UDP port 1716. BigBox both announces itself (so phones
//! dial in) and listens for phone announcements (so it can dial out). On
//! hearing a phone it has no link to, it initiates the TLS link.

use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;

use crate::connection;
use crate::identity::{Identity, PACKET_TYPE_IDENTITY};
use crate::packet::NetworkPacket;
use crate::{EngineInner, DISCOVERY_PORT};

/// Listen for phone identity broadcasts and dial out to new ones.
pub async fn recv_loop(inner: Arc<EngineInner>) {
    let sock = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).await {
        Ok(s) => s,
        // Port likely held by a running kdeconnectd; inbound discovery is then
        // unavailable but our own broadcast still lets phones find us.
        Err(_) => return,
    };
    let _ = sock.set_broadcast(true);

    let mut buf = vec![0u8; 8192];
    loop {
        let (n, src) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let text = String::from_utf8_lossy(&buf[..n]);
        let ident = match parse(&text) {
            Some(i) => i,
            None => continue,
        };
        if ident.device_id == inner.device_id() {
            continue;
        }
        if inner.links.lock().unwrap().contains_key(&ident.device_id) {
            continue;
        }
        {
            let mut connecting = inner.connecting.lock().unwrap();
            if !connecting.insert(ident.device_id.clone()) {
                continue; // a dial is already in flight
            }
        }

        let port = ident.tcp_port.unwrap_or(DISCOVERY_PORT);
        let ip = src.ip();
        let inner2 = inner.clone();
        tokio::spawn(async move {
            let id = ident.device_id.clone();
            if connection::dial(inner2.clone(), ip, port, ident).await.is_err() {
                inner2.connecting.lock().unwrap().remove(&id);
            }
        });
    }
}

/// Periodically broadcast our identity so phones can dial us.
pub async fn broadcast_loop(inner: Arc<EngineInner>) {
    let sock = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = sock.set_broadcast(true);

    loop {
        let port = inner.tcp_port.load(Ordering::Relaxed);
        if port != 0 {
            if let Ok(line) = inner.identity(Some(port)).to_packet().to_line() {
                let _ = sock
                    .send_to(line.as_bytes(), (Ipv4Addr::BROADCAST, DISCOVERY_PORT))
                    .await;
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

fn parse(text: &str) -> Option<Identity> {
    let pkt = NetworkPacket::from_line(text.trim()).ok()?;
    if pkt.packet_type != PACKET_TYPE_IDENTITY {
        return None;
    }
    serde_json::from_value(pkt.body).ok()
}
