// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-core` — the shared domain vocabulary for BigBox.
//!
//! Pure data types, IDs, and UI constants. Everything else depends inward on
//! this crate; it depends on nothing internal. Keep it cheap: serde/chrono/uuid
//! only — no tauri, tokio, reqwest, toml, or dirs.

pub mod config;
pub mod layout;
pub mod services;
pub mod vorcaro;
