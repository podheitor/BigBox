// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! WhatsApp Cloud API sender — re-exported from the `bigbox-cloud` crate.
//! Moved out so the reqwest/HTTP layer compiles independently of the Tauri glue.
//! This shim keeps `crate::vorcaro::cloud_api::*` paths valid
//! (`WhatsAppCloudConfig`, `TemplateInfo`, `load_config`, `save_config`,
//! `verify_connection`, `list_templates`, `send_text`, `send_template`).

pub use bigbox_cloud::*;
