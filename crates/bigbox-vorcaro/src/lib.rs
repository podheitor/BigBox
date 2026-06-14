// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-vorcaro` — Vorcaro's Studio Tauri IPC: contacts, lists, tags, CSV
//! import, settings, scraping, and campaigns. The Tauri-edge crate for the CRM;
//! a sibling of `bigbox-shell` (no dependency between them). It owns the port
//! *adapters* (`TauriTransport`/`TauriProgress`) that bridge the Tauri-free
//! `bigbox-orchestrator` engine to the live `AppHandle`.
//!
//! The submodules below are thin re-exports of the extracted crates, kept so
//! the original `model::`/`orchestrator::`/`cloud_api::`/`csv_io::` paths in
//! the IPC code still resolve. The IPC commands live in the `ipc` module (not
//! the crate root) because `#[tauri::command]` generates a `__cmd__*` macro
//! that collides with its own re-export at a library's crate root.

pub mod attachments;
pub mod cloud_api;
pub mod csv_io;
pub mod drivers;
pub mod model;
pub mod orchestrator;
pub mod store;

mod ipc;
pub use ipc::*;
