// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Config: load/save AppConfig from ~/.config/bigbox/config.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserService {
    pub id:           String,
    pub service_type: String,
    pub display_name: String,
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

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bigbox")
        .join("config.toml")
}

pub fn load() -> AppConfig {
    let path = config_path();
    if !path.exists() {
        return AppConfig::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    toml::from_str(&text).unwrap_or_default()
}

pub fn save(cfg: &AppConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = toml::to_string_pretty(cfg).unwrap_or_default();
    let _ = std::fs::write(&path, text);
}
