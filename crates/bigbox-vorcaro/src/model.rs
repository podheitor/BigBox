// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Vorcaro's Studio data model — re-exported from `bigbox-core`.
//! The types moved to the leaf crate so they can be shared without dragging in
//! the Tauri/IPC layer. This shim keeps `crate::vorcaro::model::*` paths valid.

pub use bigbox_core::vorcaro::*;
