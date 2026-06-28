// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! KDE Connect SMS plugin: request/send packet builders and parsing of the
//! `kdeconnect.sms.messages` replies into [`bigbox_core::sms`] types.
//!
//! Android message `type`: 1 = inbox (received), 2 = sent. A
//! `request_conversations` reply carries the latest message of every thread;
//! a `request_conversation` reply carries many messages for one thread; an
//! unsolicited push carries a single newly-arrived message.

use bigbox_core::sms::{Conversation, SmsAddress, SmsMessage};
use serde_json::{json, Value};

use crate::packet::NetworkPacket;

pub const TYPE_REQUEST_CONVERSATIONS: &str = "kdeconnect.sms.request_conversations";
pub const TYPE_REQUEST_CONVERSATION: &str = "kdeconnect.sms.request_conversation";
pub const TYPE_REQUEST: &str = "kdeconnect.sms.request";
pub const TYPE_MESSAGES: &str = "kdeconnect.sms.messages";

/// Ask the phone for the latest message of every conversation.
pub fn request_conversations() -> NetworkPacket {
    NetworkPacket::new(TYPE_REQUEST_CONVERSATIONS, json!({}))
}

/// Ask the phone for the messages of one thread (whole history).
pub fn request_conversation(thread_id: i64) -> NetworkPacket {
    NetworkPacket::new(
        TYPE_REQUEST_CONVERSATION,
        json!({
            "threadID": thread_id,
            "rangeStartTimestamp": -1,
            "numberToRequest": -1,
        }),
    )
}

/// Build a send-SMS request to one or more addresses.
pub fn send_sms(addresses: &[String], body: &str) -> NetworkPacket {
    let addrs: Vec<Value> = addresses.iter().map(|a| json!({ "address": a })).collect();
    NetworkPacket::new(
        TYPE_REQUEST,
        json!({
            "version": 2,
            "sendSms": true,
            "addresses": addrs,
            "messageBody": body,
        }),
    )
}

/// Parse the `messages` array of a `kdeconnect.sms.messages` body.
pub fn parse_messages(body: &Value) -> Vec<SmsMessage> {
    body.get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.iter().filter_map(parse_one).collect())
        .unwrap_or_default()
}

fn parse_one(m: &Value) -> Option<SmsMessage> {
    let thread_id = m.get("thread_id").and_then(value_i64).unwrap_or(-1);
    let body = m
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let date = m.get("date").and_then(value_i64).unwrap_or(0);
    // type 2 == sent by the phone owner.
    let from_me = m.get("type").and_then(value_i64).unwrap_or(1) == 2;
    let read = m
        .get("read")
        .and_then(value_i64)
        .map(|r| r != 0)
        .unwrap_or(true);
    let addresses = parse_addresses(m.get("addresses"));
    let attachment_count = m
        .get("attachments")
        .and_then(|a| a.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);

    Some(SmsMessage {
        thread_id,
        from_me,
        body,
        date,
        read,
        addresses,
        attachment_count,
    })
}

/// `addresses` may be an array of `{ "address": ".." }` objects, or a bare
/// string in older payloads.
fn parse_addresses(v: Option<&Value>) -> Vec<SmsAddress> {
    match v {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|a| {
                let address = a
                    .get("address")
                    .and_then(|s| s.as_str())
                    .or_else(|| a.as_str())?;
                Some(SmsAddress {
                    address: address.to_string(),
                    display_name: a
                        .get("display_name")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect(),
        Some(Value::String(s)) => vec![SmsAddress {
            address: s.clone(),
            display_name: None,
        }],
        _ => Vec::new(),
    }
}

/// Collapse a thread's messages into a single conversation-list row using its
/// most recent message.
pub fn conversation_from_messages(messages: &[SmsMessage]) -> Option<Conversation> {
    let latest = messages.iter().max_by_key(|m| m.date)?;
    Some(Conversation {
        thread_id: latest.thread_id,
        addresses: latest.addresses.clone(),
        snippet: latest.body.clone(),
        date: latest.date,
        read: latest.read,
        from_me: latest.from_me,
    })
}

/// Some clients send numeric fields as JSON numbers, others as strings; accept
/// both.
fn value_i64(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_messages_reply() {
        let body = json!({
            "version": 2,
            "messages": [
                {
                    "thread_id": 12,
                    "body": "hello there",
                    "date": 1690000000000i64,
                    "type": 1,
                    "read": 1,
                    "addresses": [{ "address": "+15551234567" }]
                },
                {
                    "thread_id": 12,
                    "body": "reply",
                    "date": 1690000001000i64,
                    "type": 2,
                    "read": 1,
                    "addresses": [{ "address": "+15551234567" }]
                }
            ]
        });
        let msgs = parse_messages(&body);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].thread_id, 12);
        assert!(!msgs[0].from_me, "type 1 is inbound");
        assert!(msgs[1].from_me, "type 2 is sent");
        assert_eq!(msgs[0].addresses[0].address, "+15551234567");

        let conv = conversation_from_messages(&msgs).unwrap();
        assert_eq!(conv.snippet, "reply", "latest message wins");
        assert!(conv.from_me);
    }

    #[test]
    fn handles_string_numbers_and_attachments() {
        let body = json!({
            "messages": [{
                "thread_id": "7",
                "body": "pic",
                "date": "1690000000000",
                "type": 1,
                "read": 0,
                "addresses": [{ "address": "+1999" }],
                "attachments": [{ "part_id": 1 }, { "part_id": 2 }]
            }]
        });
        let msgs = parse_messages(&body);
        assert_eq!(msgs[0].thread_id, 7);
        assert_eq!(msgs[0].date, 1690000000000);
        assert!(!msgs[0].read);
        assert_eq!(msgs[0].attachment_count, 2);
    }

    #[test]
    fn builds_a_send_packet() {
        let pkt = send_sms(&["+15550001111".to_string()], "yo");
        assert_eq!(pkt.packet_type, TYPE_REQUEST);
        assert_eq!(pkt.body["sendSms"], true);
        assert_eq!(pkt.body["messageBody"], "yo");
        assert_eq!(pkt.body["addresses"][0]["address"], "+15550001111");
    }

    #[test]
    fn packet_round_trips_through_a_line() {
        let pkt = request_conversation(42);
        let line = pkt.to_line().unwrap();
        assert!(line.ends_with('\n'));
        let back = crate::packet::NetworkPacket::from_line(line.trim()).unwrap();
        assert_eq!(back.packet_type, TYPE_REQUEST_CONVERSATION);
        assert_eq!(back.body["threadID"], 42);
    }
}
