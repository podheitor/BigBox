// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! App configuration types. The on-disk load/save lives in `bigbox-config`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserService {
    pub id:           String,
    pub service_type: String,
    pub display_name: String,
    /// Per-instance URL override for self-hosted services (e.g. Carbonio).
    /// `None` → use the catalog's default URL.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub services: Vec<UserService>,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub sidebar_collapsed: bool,
}
