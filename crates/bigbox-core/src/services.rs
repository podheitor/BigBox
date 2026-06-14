// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Service catalog type. The embedded catalog load + session-dir helpers live
//! in `bigbox-config`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDef {
    pub id:         String,
    pub name:       String,
    pub url:        String,
    #[serde(default)]
    pub color:      String,
    #[serde(default)]
    #[serde(rename = "user_agent_override")]
    pub user_agent: Option<String>,
    /// Self-hosted services (e.g. Carbonio) have no fixed URL — the user
    /// supplies their server address when adding the service.
    #[serde(default)]
    pub requires_url: bool,
}
