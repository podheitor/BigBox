// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-orchestrator` — the campaign send engine. The hot-edit crate.
//!
//! **Tauri-free by construction.** It reaches the UI and chat-service WebViews
//! only through the `DriverTransport` / `ProgressSink` ports from
//! `bigbox-contract`; the Tauri layer (`bigbox-vorcaro`) supplies the concrete
//! adapters. This keeps the most-edited code out of the Tauri/wry blast radius.

pub mod attachments;
pub mod csv_io;
mod engine;

pub use engine::*;
