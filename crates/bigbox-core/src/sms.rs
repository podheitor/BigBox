// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! SMS domain vocabulary, shared between the KDE Connect peer
//! (`bigbox-kdeconnect`), the shell IPC layer, and the frontend pane.
//!
//! Field names are camelCased on the wire so the JS pane can consume them
//! directly from the Tauri `invoke`/event payloads.

use serde::{Deserialize, Serialize};

/// A phone discovered on the LAN and/or paired with BigBox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    /// Stable KDE Connect device id (the cert/identity anchor).
    pub device_id: String,
    /// Human-facing device name advertised in the identity packet.
    pub name: String,
    /// True once the pairing handshake has completed and the cert is pinned.
    pub paired: bool,
    /// True while a live TLS connection to the device is open.
    pub reachable: bool,
}

/// One participant address in a conversation (phone number / shortcode), with
/// the contact name the phone resolved for it, when available.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsAddress {
    pub address: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// A single SMS/MMS message within a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsMessage {
    pub thread_id: i64,
    /// True if BigBox/the phone owner sent it (KDE Connect message type 2),
    /// false for received (type 1).
    pub from_me: bool,
    pub body: String,
    /// Unix epoch milliseconds.
    pub date: i64,
    pub read: bool,
    /// Sender (for inbound) or recipients (for outbound).
    pub addresses: Vec<SmsAddress>,
    /// Number of MMS attachments; 0 for a plain SMS. v1 renders the count only.
    #[serde(default)]
    pub attachment_count: u32,
}

/// One row in the conversation list: a thread plus its latest-message preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub thread_id: i64,
    pub addresses: Vec<SmsAddress>,
    /// Preview of the most recent message.
    pub snippet: String,
    /// Timestamp of the latest message (epoch milliseconds).
    pub date: i64,
    /// Whether the latest message has been read.
    pub read: bool,
    /// Whether the latest message was sent by the phone owner.
    pub from_me: bool,
}
