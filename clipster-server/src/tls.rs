use anyhow::{Context, Result};
use rcgen::{CertificateParams, KeyPair};
use sha1::Digest;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

/// Load or auto-generate TLS cert + key. Returns a TlsAcceptor.
pub fn setup(
    data_dir: &Path,
    cert_path: Option<&str>,
    key_path: Option<&str>,
) -> Result<TlsAcceptor> {
    let (cert_pem, key_pem) = match (cert_path, key_path) {
        (Some(c), Some(k)) => {
            let cert = std::fs::read_to_string(c)
                .with_context(|| format!("failed to read cert: {c}"))?;
            let key = std::fs::read_to_string(k)
                .with_context(|| format!("failed to read key: {k}"))?;
            (cert, key)
        }
        _ => {
            let cert_file = data_dir.join("cert.pem");
            let key_file = data_dir.join("key.pem");

            if cert_file.exists() && key_file.exists() {
                tracing::info!("using existing TLS cert at {}", cert_file.display());
                let cert = std::fs::read_to_string(&cert_file)?;
                let key = std::fs::read_to_string(&key_file)?;
                (cert, key)
            } else {
                tracing::info!("generating self-signed TLS certificate");
                let (cert, key) = generate_self_signed()?;
                std::fs::write(&cert_file, &cert)?;
                std::fs::write(&key_file, &key)?;
                // Restrict key file permissions
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600))?;
                }
                tracing::info!("TLS cert saved to {}", cert_file.display());
                (cert, key)
            }
        }
    };

    print_fingerprint(&cert_pem);
    build_acceptor(&cert_pem, &key_pem)
}

fn generate_self_signed() -> Result<(String, String)> {
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

fn print_fingerprint(cert_pem: &str) {
    let der = match pem_to_der(cert_pem) {
        Some(d) => d,
        None => return,
    };
    let mut hasher = sha1::Sha1::new();
    hasher.update(&der);
    let hash = hasher.finalize();
    let fingerprint: Vec<String> = hash.iter().map(|b| format!("{b:02X}")).collect();
    let fp = fingerprint.join(":");
    tracing::info!("TLS certificate fingerprint (SHA-1): {fp}");
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>().ok()?;
    certs.into_iter().next().map(|c| c.to_vec())
}

fn build_acceptor(cert_pem: &str, key_pem: &str) -> Result<TlsAcceptor> {
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
