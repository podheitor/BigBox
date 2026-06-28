// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! BigBox's own TLS identity: a self-signed EC P-256 certificate generated once
//! and persisted, used for every KDE Connect link. The certificate *is* the
//! device identity — pairing pins the peer's cert, and the phone pins ours.
//!
//! Note: the `ring` rcgen backend produces EC keys. Current KDE Connect Android
//! builds accept EC certs; very old RSA-only builds will not pair. The cert CN
//! carries the deviceId, matching KDE Connect's convention.

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// BigBox's persistent device identity material.
#[derive(Clone)]
pub struct DeviceCert {
    /// Stable KDE Connect device id (also the certificate CN).
    pub device_id: String,
    pub cert_pem: String,
    pub key_pem: String,
}

impl DeviceCert {
    /// Load the cert+key from `dir`, generating and persisting them on first
    /// run. `device_id` is only used when generating (an existing cert keeps
    /// its embedded id).
    pub fn load_or_create(dir: &Path, device_id: &str) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        let id_path = dir.join("device_id");

        if cert_path.exists() && key_path.exists() && id_path.exists() {
            let cert_pem = std::fs::read_to_string(&cert_path)?;
            let key_pem = std::fs::read_to_string(&key_path)?;
            let device_id = std::fs::read_to_string(&id_path)?.trim().to_string();
            return Ok(Self { device_id, cert_pem, key_pem });
        }

        let (cert_pem, key_pem) = generate(device_id)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        std::fs::write(&cert_path, &cert_pem)?;
        write_private(&key_path, &key_pem)?;
        std::fs::write(&id_path, device_id)?;

        Ok(Self {
            device_id: device_id.to_string(),
            cert_pem,
            key_pem,
        })
    }

    /// Parse the certificate chain (single self-signed cert) as rustls DER.
    pub fn cert_chain(&self) -> io::Result<Vec<CertificateDer<'static>>> {
        let mut rd = io::Cursor::new(self.cert_pem.as_bytes());
        rustls_pemfile::certs(&mut rd).collect::<Result<Vec<_>, _>>()
    }

    /// Parse the private key as rustls DER.
    pub fn private_key(&self) -> io::Result<PrivateKeyDer<'static>> {
        let mut rd = io::Cursor::new(self.key_pem.as_bytes());
        rustls_pemfile::private_key(&mut rd)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no private key in PEM"))
    }
}

/// SHA-256 fingerprint of a DER certificate, lowercase hex — the value pinned
/// on pairing and compared on every reconnect.
pub fn fingerprint(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest.iter() {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn generate(device_id: &str) -> Result<(String, String), rcgen::Error> {
    let mut params = CertificateParams::new(Vec::new())?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, device_id);
    dn.push(DnType::OrganizationName, "KDE");
    dn.push(DnType::OrganizationalUnitName, "KDE Connect");
    params.distinguished_name = dn;

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

#[cfg(unix)]
fn write_private(path: &PathBuf, contents: &str) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &PathBuf, contents: &str) -> io::Result<()> {
    std::fs::write(path, contents)
}
