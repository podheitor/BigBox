// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Load/save VorcaroState from ~/.config/bigbox/vorcaro.toml

use std::path::PathBuf;

use bigbox_core::vorcaro::VorcaroState;

pub fn state_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bigbox")
        .join("vorcaro.toml")
}

pub fn load() -> VorcaroState {
    let path = state_path();
    if !path.exists() {
        return VorcaroState::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    toml::from_str(&text).unwrap_or_default()
}

pub fn save(state: &VorcaroState) -> Result<(), String> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}
