// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Vorcaro state persistence — re-exported from `bigbox-config`. This shim keeps
//! `crate::vorcaro::store::{load, save, state_path}` paths valid.

pub use bigbox_config::store::*;
