// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Persistent trust store: which phones BigBox has paired with, and the
//! certificate fingerprint pinned for each. A device is trusted iff it appears
//! here with a fingerprint matching the cert it presents on reconnect.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub device_id: String,
    pub name: String,
    /// SHA-256 hex fingerprint of the device's TLS cert, pinned at pairing.
    pub cert_fingerprint: String,
}

#[derive(Default)]
pub struct TrustStore {
    path: PathBuf,
    devices: HashMap<String, TrustedDevice>,
}

impl TrustStore {
    /// Load the trust store from `path` (missing file → empty store).
    pub fn load(path: PathBuf) -> Self {
        let devices = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<TrustedDevice>>(&s).ok())
            .map(|v| v.into_iter().map(|d| (d.device_id.clone(), d)).collect())
            .unwrap_or_default();
        Self { path, devices }
    }

    fn persist(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let list: Vec<&TrustedDevice> = self.devices.values().collect();
        let json = serde_json::to_string_pretty(&list)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(&self.path, json)
    }

    pub fn is_paired(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    /// The pinned fingerprint for a paired device, if any.
    pub fn fingerprint(&self, device_id: &str) -> Option<&str> {
        self.devices.get(device_id).map(|d| d.cert_fingerprint.as_str())
    }

    /// Pin (or re-pin) a device on successful pairing.
    pub fn pin(&mut self, device_id: &str, name: &str, fingerprint: &str) -> io::Result<()> {
        self.devices.insert(
            device_id.to_string(),
            TrustedDevice {
                device_id: device_id.to_string(),
                name: name.to_string(),
                cert_fingerprint: fingerprint.to_string(),
            },
        );
        self.persist()
    }

    /// Forget a device (unpair).
    pub fn remove(&mut self, device_id: &str) -> io::Result<()> {
        self.devices.remove(device_id);
        self.persist()
    }

    pub fn list(&self) -> Vec<TrustedDevice> {
        self.devices.values().cloned().collect()
    }
}

/// Convenience: trust-store path under a base config dir.
pub fn store_path(base: &Path) -> PathBuf {
    base.join("trusted_devices.json")
}
