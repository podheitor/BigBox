// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! `bigbox-config` — all on-disk persistence + embedded assets for BigBox.
//!
//! Owns: app config (`config.toml`), the embedded services catalog
//! (`services.json`) + session-dir helper, and Vorcaro state (`vorcaro.toml`).
//! Types come from `bigbox-core`; this crate adds the toml/json/dirs I/O.

pub mod config;
pub mod services;
pub mod store;
