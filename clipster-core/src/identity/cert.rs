use anyhow::{Context, Result};
use rcgen::{CertificateParams, KeyPair};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// Generate a self-signed certificate + key for this device.
pub fn generate_self_signed() -> Result<(String, String)> {
    let mut params = CertificateParams::new(vec![
        "localhost".to_string(),
        "clipster".to_string(),
    ])?;

    // Add SANs for common LAN access patterns
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
    params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::new(0, 0, 0, 0),
        )));

    // Valid for 10 years
    params.not_after = rcgen::date_time_ymd(2036, 1, 1);

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    Ok((cert.pem(), key_pair.serialize_pem()))
}

/// SHA-256 hex of the DER-encoded certificate. This is the stable device identifier.
pub fn sha256_fingerprint(cert_pem: &str) -> Option<String> {
    let der = pem_to_der(cert_pem)?;
    let mut hasher = Sha256::new();
    hasher.update(&der);
    Some(hex::encode(hasher.finalize()))
}

/// Pretty-printed colon-separated SHA-256 fingerprint (for log display).
pub fn pretty_fingerprint(hex_fp: &str) -> String {
    hex_fp
        .as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap_or("?").to_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

pub fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    certs.into_iter().next().map(|c| c.to_vec())
}

pub fn build_acceptor(cert_pem: &str, key_pem: &str) -> Result<TlsAcceptor> {
    // Ensure a crypto provider is installed (reqwest may not have done it yet)
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut cert_reader = std::io::BufReader::new(cert_pem.as_bytes());
    let mut key_reader = std::io::BufReader::new(key_pem.as_bytes());

    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse TLS cert")?;

    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("failed to parse TLS key")?
        .context("no private key found")?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("invalid TLS cert/key")?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_64_hex_chars() {
        let (cert, _) = generate_self_signed().unwrap();
        let fp = sha256_fingerprint(&cert).unwrap();
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fingerprint_stable_for_same_cert() {
        let (cert, _) = generate_self_signed().unwrap();
        let fp1 = sha256_fingerprint(&cert).unwrap();
        let fp2 = sha256_fingerprint(&cert).unwrap();
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_differs_for_different_certs() {
        let (cert1, _) = generate_self_signed().unwrap();
        let (cert2, _) = generate_self_signed().unwrap();
        assert_ne!(
            sha256_fingerprint(&cert1).unwrap(),
            sha256_fingerprint(&cert2).unwrap()
        );
    }
}
