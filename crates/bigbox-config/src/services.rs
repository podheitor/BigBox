// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Service catalog: load built-in service definitions from embedded JSON.

pub use bigbox_core::services::ServiceDef;

/// Embedded services catalog (compiled into the binary)
const CATALOG_JSON: &str = include_str!("../data/services.json");

pub fn load_catalog() -> Vec<ServiceDef> {
    serde_json::from_str(CATALOG_JSON).unwrap_or_default()
}

/// Session data dir for a service instance
pub fn session_dir(service_id: &str) -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("bigbox")
        .join("sessions")
        .join(service_id)
}
