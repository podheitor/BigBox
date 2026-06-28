// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! The KDE Connect wire envelope: newline-delimited JSON packets of the shape
//! `{ "id": <millis>, "type": "kdeconnect.*", "body": { .. } }`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// KDE Connect protocol version BigBox speaks. v7 is the cert-trust era used by
/// current Android clients. Bump only after re-validating against the app.
pub const PROTOCOL_VERSION: i32 = 7;

/// One framed packet on the link. Serializes to a single JSON line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPacket {
    pub id: i64,
    #[serde(rename = "type")]
    pub packet_type: String,
    pub body: Value,
    /// Present only for packets carrying a binary payload (MMS attachments).
    /// v1 never sends these; we tolerate them on receive.
    #[serde(rename = "payloadSize", skip_serializing_if = "Option::is_none", default)]
    pub payload_size: Option<i64>,
    #[serde(
        rename = "payloadTransferInfo",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub payload_transfer_info: Option<Value>,
}

impl NetworkPacket {
    /// Build a packet with a fresh millisecond id and the given typed body.
    pub fn new(packet_type: impl Into<String>, body: Value) -> Self {
        Self {
            id: now_ms(),
            packet_type: packet_type.into(),
            body,
            payload_size: None,
            payload_transfer_info: None,
        }
    }

    /// Serialize to a single newline-terminated JSON line (the wire framing).
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(self)?;
        s.push('\n');
        Ok(s)
    }

    /// Parse one JSON line (newline already stripped) into a packet.
    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }
}

/// Wall-clock milliseconds, used for packet ids (KDE Connect treats id as an
/// opaque monotonic-ish timestamp). Falls back to 0 before the epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
