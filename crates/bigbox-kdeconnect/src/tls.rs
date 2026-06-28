// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Heitor Faria

//! Mutual-TLS configs for KDE Connect links. KDE Connect authenticates peers by
//! pinning self-signed certificates (trust-on-first-use), NOT by a CA chain, so
//! both verifiers accept any presented certificate at the TLS layer; the app
//! layer ([`crate::pairing`]) compares the SHA-256 fingerprint against the
//! trust store before honoring any SMS traffic.

use std::sync::Arc;

use tokio_rustls::rustls::{
    self,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{ring, verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
};

use crate::cert::DeviceCert;

fn algs() -> WebPkiSupportedAlgorithms {
    ring::default_provider().signature_verification_algorithms
}

/// Client-side verifier that trusts any server cert (pinning happens above).
#[derive(Debug)]
struct AcceptAnyServer {
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for AcceptAnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Server-side verifier that requests and trusts any client cert (so we can
/// read its fingerprint), with client auth mandatory — KDE Connect is mTLS.
#[derive(Debug)]
struct AcceptAnyClient {
    algs: WebPkiSupportedAlgorithms,
}

impl ClientCertVerifier for AcceptAnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// Build a rustls `ClientConfig` presenting our device cert. Used when BigBox
/// *accepts* a TCP connection (the connection initiator is the TLS server).
pub fn client_config(cert: &DeviceCert) -> Result<Arc<ClientConfig>, rustls::Error> {
    let provider = Arc::new(ring::default_provider());
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyServer { algs: algs() }))
        .with_client_auth_cert(
            cert.cert_chain().map_err(io_to_rustls)?,
            cert.private_key().map_err(io_to_rustls)?,
        )?;
    Ok(Arc::new(cfg))
}

/// Build a rustls `ServerConfig` presenting our device cert and demanding a
/// client cert. Used when BigBox *initiates* a TCP connection (the initiator is
/// the TLS server).
pub fn server_config(cert: &DeviceCert) -> Result<Arc<ServerConfig>, rustls::Error> {
    let provider = Arc::new(ring::default_provider());
    let cfg = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_client_cert_verifier(Arc::new(AcceptAnyClient { algs: algs() }))
        .with_single_cert(
            cert.cert_chain().map_err(io_to_rustls)?,
            cert.private_key().map_err(io_to_rustls)?,
        )?;
    Ok(Arc::new(cfg))
}

fn io_to_rustls(e: std::io::Error) -> rustls::Error {
    rustls::Error::General(e.to_string())
}
