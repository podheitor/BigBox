// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Phase E.2: campaign attachments.
//!
//! Files staged here live under `~/.cache/bigbox/vorcaro/attachments/`. The
//! frontend reads each user-picked file via FileReader, base64-encodes it, and
//! hands the bytes to Rust via `vorcaro_stage_attachment`. We persist them on
//! disk and put the path into the Campaign record. The orchestrator reads them
//! back at send time, re-encodes to base64, and passes them through `wv.eval()`
//! to the driver, which synthesizes a `File` and injects it into WhatsApp /
//! Telegram Web's hidden file input.
//!
//! We do not put base64 directly in `vorcaro.toml` — TOML bloat aside, that
//! also defeats deduplication across campaigns. Files are content-hashed
//! filenames, so the same image used twice doesn't take two copies.

use std::path::PathBuf;

use base64::Engine;

const SUBDIR: &str = "bigbox/vorcaro/attachments";

fn staging_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(SUBDIR)
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect()
}

/// Stage a file under the cache dir. Returns the absolute path the orchestrator
/// can read at send time.
pub fn stage(name: &str, b64: &str) -> Result<PathBuf, String> {
    let dir = staging_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("decode base64: {e}"))?;

    // Use a uuid prefix so two attachments with the same display name don't clash.
    let safe = sanitize_name(name);
    let prefix = uuid::Uuid::new_v4();
    let target = dir.join(format!("{prefix}-{safe}"));
    std::fs::write(&target, bytes).map_err(|e| format!("write: {e}"))?;
    Ok(target)
}

/// Read a previously-staged file back as base64. Returns `(name, mime, b64)`.
pub fn read_as_base64(path: &std::path::Path) -> Result<(String, String, String), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| {
            // Strip the uuid prefix added by stage(): `<uuid>-<original>`
            n.splitn(2, '-').nth(1).unwrap_or(n).to_string()
        })
        .unwrap_or_else(|| "file".into());
    let mime = guess_mime(&name);
    Ok((name, mime, b64))
}

/// Garbage-collect files no longer referenced by any campaign. Called whenever
/// a campaign ends (Done / Aborted). Cheap walk; tolerates errors.
pub fn gc_unreferenced(referenced: &std::collections::HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(staging_dir()) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if !referenced.contains(&p) {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn guess_mime(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "txt" => "text/plain",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
    .to_string()
}
