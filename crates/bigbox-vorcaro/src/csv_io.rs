// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! CSV import — re-exported from `bigbox-orchestrator` (engine-side IO). This
//! shim keeps `crate::vorcaro::csv_io::*` paths valid (`import_csv`,
//! `ImportReport`, `ImportReportSerde`).

pub use bigbox_orchestrator::csv_io::*;
