// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-shell` — Tauri IPC commands: config CRUD + the embedded service
//! WebView host, plus the GTK overlay layout that positions those webviews.
//! One of the two Tauri-edge crates; a sibling of `bigbox-vorcaro` (neither
//! depends on the other — they join only at the app crate).
//!
//! The IPC commands live in the `commands` module (not the crate root) because
//! `#[tauri::command]` generates a `__cmd__*` macro that collides with its own
//! re-export when placed at a library's crate root.

mod commands;
pub use commands::*;
