// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! The `kdeconnect.identity` packet exchanged at the start of every link, plus
//! the capability set that declares BigBox an SMS-capable peer.

use crate::packet::{NetworkPacket, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PACKET_TYPE_IDENTITY: &str = "kdeconnect.identity";

/// SMS plugin packet types BigBox can *send* to the phone.
pub const OUTGOING_CAPS: &[&str] = &[
    "kdeconnect.sms.request",
    "kdeconnect.sms.request_conversations",
    "kdeconnect.sms.request_conversation",
];

/// SMS plugin packet types BigBox accepts *from* the phone.
pub const INCOMING_CAPS: &[&str] = &[
    "kdeconnect.sms.messages",
    "kdeconnect.telephony",
];

/// Parsed body of an identity packet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub device_id: String,
    pub device_name: String,
    #[serde(default = "desktop")]
    pub device_type: String,
    #[serde(default)]
    pub protocol_version: i32,
    /// TCP port the announcer is listening on for the link upgrade.
    #[serde(default)]
    pub tcp_port: Option<u16>,
    #[serde(default)]
    pub incoming_capabilities: Vec<String>,
    #[serde(default)]
    pub outgoing_capabilities: Vec<String>,
}

fn desktop() -> String {
    "desktop".to_string()
}

impl Identity {
    /// BigBox's own identity, advertised on discovery and over each link.
    /// `tcp_port` is `Some` only in the UDP broadcast (where the phone needs to
    /// know where to connect back); `None` over an established TCP link.
    pub fn this_device(device_id: &str, device_name: &str, tcp_port: Option<u16>) -> Self {
        Self {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            device_type: desktop(),
            protocol_version: PROTOCOL_VERSION,
            tcp_port,
            incoming_capabilities: INCOMING_CAPS.iter().map(|s| s.to_string()).collect(),
            outgoing_capabilities: OUTGOING_CAPS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Wrap this identity in a `kdeconnect.identity` packet.
    pub fn to_packet(&self) -> NetworkPacket {
        let body = serde_json::to_value(self).unwrap_or(Value::Null);
        NetworkPacket::new(PACKET_TYPE_IDENTITY, body)
    }

    /// True if the peer advertises that it can serve SMS conversations.
    pub fn supports_sms(&self) -> bool {
        self.outgoing_capabilities
            .iter()
            .any(|c| c == "kdeconnect.sms.messages")
            || self
                .incoming_capabilities
                .iter()
                .any(|c| c == "kdeconnect.sms.request")
    }
}
