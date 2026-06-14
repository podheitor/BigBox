// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Campaign engine — re-exported from the Tauri-free `bigbox-orchestrator`
//! crate. The engine talks to WebViews / the UI through the `DriverTransport`
//! and `ProgressSink` ports (see `crate::vorcaro` for the Tauri adapters that
//! implement them). This shim keeps `crate::vorcaro::orchestrator::*` valid
//! (`OrchestratorState`, `SendOutcome`, `start`, `pause`, `resume`, `abort`,
//! `route_send_result`, `resolve_targets`).

pub use bigbox_orchestrator::*;
