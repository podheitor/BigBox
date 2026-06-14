// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Campaign attachments — re-exported from `bigbox-orchestrator` (engine-side
//! IO). This shim keeps `crate::vorcaro::attachments::*` paths valid
//! (`stage`, `read_as_base64`, `gc_unreferenced`).

pub use bigbox_orchestrator::attachments::*;
