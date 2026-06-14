// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Driver scripts — re-exported from the `bigbox-driver-assets` leaf crate.
//! The JS blobs moved to `assets/*.js` so iterating on a driver only recompiles
//! that trivial crate, never the Tauri/wry glue. This shim keeps
//! `crate::vorcaro::drivers::VORCARO_*` paths valid.

pub use bigbox_driver_assets::{VORCARO_TELEGRAM_DRIVER, VORCARO_WHATSAPP_DRIVER};
