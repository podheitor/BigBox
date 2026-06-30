// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Resolve contact names + photos from KDE Connect's vCard cache. The contacts
//! plugin syncs the phone's address book to
//! `~/.local/share/kpeoplevcard/kdeconnect-<deviceId>/*.vcf` (once the phone
//! grants the Contacts permission). We parse those vCards directly — no D-Bus.

use bigbox_core::sms::Contact;
use std::path::Path;

/// Load contacts for a device from two sources:
/// 1. KDE Connect's synced vCard cache (`~/.local/share/kpeoplevcard/...`) —
///    works only if the phone's contacts plugin actually syncs.
/// 2. A user-managed folder `~/.config/bigbox/contacts/*.vcf` — drop an
///    exported address book here and names/photos work even when KDE Connect's
///    contacts sync is broken (a single export file may hold many vCards).
pub fn load_contacts(device_id: &str) -> Vec<Contact> {
    let mut out = Vec::new();
    if let Some(d) = dirs::data_dir() {
        load_dir(&d.join("kpeoplevcard").join(format!("kdeconnect-{device_id}")), &mut out);
    }
    if let Some(c) = dirs::config_dir() {
        load_dir(&c.join("bigbox").join("contacts"), &mut out);
    }
    out
}

/// Cheap check (no parsing) for whether any contacts are cached — used to stop
/// re-triggering KDE Connect's contacts sync once we have data.
pub fn has_any_contacts(device_id: &str) -> bool {
    fn dir_has_vcf(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten().any(|e| {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("vcf"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }
    let kde = dirs::data_dir()
        .map(|d| dir_has_vcf(&d.join("kpeoplevcard").join(format!("kdeconnect-{device_id}"))))
        .unwrap_or(false);
    let user = dirs::config_dir()
        .map(|c| dir_has_vcf(&c.join("bigbox").join("contacts")))
        .unwrap_or(false);
    kde || user
}

fn load_dir(dir: &Path, out: &mut Vec<Contact>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        let is_vcf = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("vcf"))
            .unwrap_or(false);
        if !is_vcf {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            for card in split_vcards(&text) {
                if let Some(c) = parse_vcard(&card) {
                    if !c.numbers.is_empty() {
                        out.push(c);
                    }
                }
            }
        }
    }
}

/// Split a (possibly multi-contact) .vcf into individual vCards.
fn split_vcards(text: &str) -> Vec<String> {
    let mut cards = Vec::new();
    let mut cur = String::new();
    let mut in_card = false;
    for line in text.lines() {
        let upper = line.trim_start().to_ascii_uppercase();
        if upper.starts_with("BEGIN:VCARD") {
            in_card = true;
            cur.clear();
        }
        if in_card {
            cur.push_str(line);
            cur.push('\n');
        }
        if upper.starts_with("END:VCARD") && in_card {
            cards.push(std::mem::take(&mut cur));
            in_card = false;
        }
    }
    cards
}

/// Parse a single vCard into a [`Contact`] (name + numbers + optional photo).
pub fn parse_vcard(text: &str) -> Option<Contact> {
    let lines = unfold(text);
    let mut fn_name: Option<String> = None;
    let mut n_name: Option<String> = None;
    let mut numbers = Vec::new();
    let mut photo: Option<String> = None;

    for line in &lines {
        let Some(colon) = line.find(':') else { continue };
        let head = &line[..colon];
        let value = &line[colon + 1..];
        let mut parts = head.split(';');
        let key = parts.next().unwrap_or("").to_ascii_uppercase();
        let params: Vec<String> = parts.map(|s| s.to_ascii_uppercase()).collect();

        match key.as_str() {
            "FN" => fn_name = Some(value.trim().to_string()),
            "N" => {
                // N: Last;First;Middle;Prefix;Suffix
                let f: Vec<&str> = value.split(';').collect();
                let first = f.get(1).map(|s| s.trim()).unwrap_or("");
                let last = f.first().map(|s| s.trim()).unwrap_or("");
                let name = format!("{first} {last}").trim().to_string();
                if !name.is_empty() {
                    n_name = Some(name);
                }
            }
            "TEL" => {
                let num = value.trim().to_string();
                if !num.is_empty() {
                    numbers.push(num);
                }
            }
            "PHOTO" if photo.is_none() => {
                let v = value.trim();
                if v.starts_with("data:") {
                    photo = Some(v.to_string());
                } else {
                    let data: String = value.split_whitespace().collect();
                    if data.len() > 32 {
                        let mime = if params.iter().any(|p| p.contains("PNG")) {
                            "image/png"
                        } else if params.iter().any(|p| p.contains("GIF")) {
                            "image/gif"
                        } else {
                            "image/jpeg"
                        };
                        photo = Some(format!("data:{mime};base64,{data}"));
                    }
                }
            }
            _ => {}
        }
    }

    let name = fn_name.or(n_name)?;
    Some(Contact { name, numbers, photo })
}

/// vCard line unfolding: a line starting with a space/tab continues the
/// previous one (used heavily for long base64 PHOTO values).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_numbers_photo() {
        let vc = "BEGIN:VCARD\r\nVERSION:2.1\r\nFN:John Doe\r\nN:Doe;John;;;\r\n\
TEL;CELL:+1 (305) 555-1234\r\nTEL;HOME:305-555-9999\r\n\
PHOTO;ENCODING=BASE64;JPEG:/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAAA\r\n AAAAAAAAAA\r\nEND:VCARD\r\n";
        let c = parse_vcard(vc).unwrap();
        assert_eq!(c.name, "John Doe");
        assert_eq!(c.numbers.len(), 2);
        assert_eq!(c.numbers[0], "+1 (305) 555-1234");
        let p = c.photo.unwrap();
        assert!(p.starts_with("data:image/jpeg;base64,/9j/4AAQ"));
        // folded continuation lines joined into the base64 value
        assert!(p.ends_with("AAAAAAAAAA"));
    }

    #[test]
    fn splits_a_multi_contact_export() {
        let vc = "BEGIN:VCARD\nFN:Ana\nTEL:111\nEND:VCARD\n\
BEGIN:VCARD\nFN:Bruno\nTEL:222\nEND:VCARD\n";
        let cards = split_vcards(vc);
        assert_eq!(cards.len(), 2);
        assert_eq!(parse_vcard(&cards[0]).unwrap().name, "Ana");
        assert_eq!(parse_vcard(&cards[1]).unwrap().name, "Bruno");
    }

    #[test]
    fn falls_back_to_n_when_no_fn() {
        let vc = "BEGIN:VCARD\nN:Silva;Maria;;;\nTEL:1199998888\nEND:VCARD\n";
        let c = parse_vcard(vc).unwrap();
        assert_eq!(c.name, "Maria Silva");
    }
}
