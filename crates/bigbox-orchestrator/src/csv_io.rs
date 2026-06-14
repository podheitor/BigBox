// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! CSV import for contacts.
//!
//! Accepted columns (case-insensitive, any subset, in any order):
//!   display_name | name
//!   whatsapp     | phone | wa
//!   whatsapp_business | wa_business
//!   telegram    | tg | username
//!   tags        (comma-separated within the cell, or `;`-separated)
//!   notes
//!
//! Rows with no contact handle at all (no WA, no WA-Business, no TG) are skipped.

use std::io::Read;

use uuid::Uuid;

use bigbox_core::vorcaro::{Contact, ContactSource};

#[derive(Debug, Default, serde::Serialize)]
pub struct ImportReport {
    pub added: u32,
    pub merged: u32,
    pub skipped: u32,
}

/// Alias kept for backwards-compatibility with the IPC layer; identical type.
pub type ImportReportSerde = ImportReport;

pub fn import_csv<R: Read>(
    reader: R,
    existing: &mut Vec<Contact>,
) -> Result<ImportReport, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(reader);

    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| format!("read headers: {e}"))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();

    let idx = |names: &[&str]| -> Option<usize> {
        names.iter().find_map(|n| headers.iter().position(|h| h == n))
    };

    let i_name = idx(&["display_name", "name"]);
    let i_wa = idx(&["whatsapp", "phone", "wa"]);
    let i_wab = idx(&["whatsapp_business", "wa_business"]);
    let i_tg = idx(&["telegram", "tg", "username"]);
    let i_tags = idx(&["tags"]);
    let i_notes = idx(&["notes"]);

    let mut report = ImportReport::default();

    for rec in rdr.records() {
        let rec = match rec {
            Ok(r) => r,
            Err(_) => { report.skipped += 1; continue; }
        };

        let get = |i: Option<usize>| -> Option<String> {
            i.and_then(|i| rec.get(i)).map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string())
        };

        let wa = get(i_wa).map(normalize_phone);
        let wab = get(i_wab).map(normalize_phone);
        let tg = get(i_tg).map(normalize_telegram_handle);
        let name = get(i_name);
        let tags = get(i_tags)
            .map(|s| s.split(|c| c == ',' || c == ';')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>())
            .unwrap_or_default();
        let notes = get(i_notes);

        if wa.is_none() && wab.is_none() && tg.is_none() {
            report.skipped += 1;
            continue;
        }

        let display_name = name.unwrap_or_else(|| {
            wa.clone().or_else(|| wab.clone()).or_else(|| tg.clone()).unwrap_or_else(|| "(unnamed)".into())
        });

        // Dedup: same WA OR same WA-Business OR same TG → merge, not append.
        let existing_idx = existing.iter().position(|c| {
            (wa.is_some() && c.whatsapp == wa)
                || (wab.is_some() && c.whatsapp_business == wab)
                || (tg.is_some() && c.telegram == tg)
        });

        if let Some(i) = existing_idx {
            let c = &mut existing[i];
            if c.whatsapp.is_none() && wa.is_some() { c.whatsapp = wa; }
            if c.whatsapp_business.is_none() && wab.is_some() { c.whatsapp_business = wab; }
            if c.telegram.is_none() && tg.is_some() { c.telegram = tg; }
            for t in tags { if !c.tags.contains(&t) { c.tags.push(t); } }
            if c.notes.is_none() && notes.is_some() { c.notes = notes; }
            report.merged += 1;
        } else {
            existing.push(Contact {
                id: Uuid::new_v4(),
                display_name,
                whatsapp: wa,
                whatsapp_business: wab,
                telegram: tg,
                tags,
                source: ContactSource::Imported,
                notes,
            });
            report.added += 1;
        }
    }

    Ok(report)
}

/// Strip everything but digits and a leading `+`. WhatsApp deep-link wants E.164.
fn normalize_phone(raw: String) -> String {
    let trimmed = raw.trim();
    let has_plus = trimmed.starts_with('+');
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();
    if has_plus { format!("+{digits}") } else { digits }
}

/// `@user` → `@user`, `user` → `@user`, phone-like → leave as digits.
fn normalize_telegram_handle(raw: String) -> String {
    let t = raw.trim();
    if t.starts_with('+') || t.chars().all(|c| c.is_ascii_digit()) {
        return normalize_phone(t.to_string());
    }
    if let Some(stripped) = t.strip_prefix('@') {
        format!("@{}", stripped)
    } else {
        format!("@{}", t)
    }
}
