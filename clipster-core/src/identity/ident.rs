use super::cert;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio_rustls::TlsAcceptor;

/// Per-device identity: cert+key+derived fingerprint and human name.
pub struct Identity {
    pub device_id: String,    // SHA-256 hex of cert DER
    pub device_name: String,  // hostname (overridable)
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl Identity {
    /// Load existing identity or generate a new one.
    ///
    /// Layout: `<data_dir>/identity/{cert,key}.pem`. If a legacy
    /// `<data_dir>/{cert,key}.pem` exists (from pre-P2P versions), it is moved
    /// into the new location before loading.
    pub fn load_or_create(data_dir: &Path, device_name: Option<String>) -> Result<Self> {
        let identity_dir = data_dir.join("identity");
        std::fs::create_dir_all(&identity_dir)
            .with_context(|| format!("creating {}", identity_dir.display()))?;

        let cert_path = identity_dir.join("cert.pem");
        let key_path = identity_dir.join("key.pem");

        // Legacy migration: move old cert/key if present at the data_dir root.
        let legacy_cert = data_dir.join("cert.pem");
        let legacy_key = data_dir.join("key.pem");
        if legacy_cert.exists() && !cert_path.exists() {
            tracing::info!("migrating legacy cert {} -> {}", legacy_cert.display(), cert_path.display());
            std::fs::rename(&legacy_cert, &cert_path)?;
        }
        if legacy_key.exists() && !key_path.exists() {
            std::fs::rename(&legacy_key, &key_path)?;
        }

        let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
            let cert = std::fs::read_to_string(&cert_path)?;
            let key = std::fs::read_to_string(&key_path)?;
            (cert, key)
        } else {
            tracing::info!("generating new device identity");
            let (cert, key) = cert::generate_self_signed()?;
            std::fs::write(&cert_path, &cert)?;
            std::fs::write(&key_path, &key)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
            }
            (cert, key)
        };

        let device_id = cert::sha256_fingerprint(&cert_pem)
            .context("failed to compute device fingerprint")?;
        let device_name = device_name.unwrap_or_else(|| {
            hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| "unknown".to_string())
        });

        tracing::info!(
            device_id = %device_id,
            device_name = %device_name,
            fingerprint = %cert::pretty_fingerprint(&device_id),
            "device identity loaded"
        );

        Ok(Self {
            device_id,
            device_name,
            cert_pem,
            key_pem,
            cert_path,
            key_path,
        })
    }

    /// Build a TLS acceptor for this identity's cert+key.
    pub fn tls_acceptor(&self) -> Result<TlsAcceptor> {
        cert::build_acceptor(&self.cert_pem, &self.key_pem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_new_identity_in_fresh_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let id = Identity::load_or_create(tmp.path(), Some("test-device".into())).unwrap();
        assert_eq!(id.device_name, "test-device");
        assert_eq!(id.device_id.len(), 64);
        assert!(tmp.path().join("identity/cert.pem").exists());
        assert!(tmp.path().join("identity/key.pem").exists());
    }

    #[test]
    fn reuses_existing_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let id1 = Identity::load_or_create(tmp.path(), Some("a".into())).unwrap();
        let id2 = Identity::load_or_create(tmp.path(), Some("a".into())).unwrap();
        assert_eq!(id1.device_id, id2.device_id);
    }

    #[test]
    fn migrates_legacy_cert() {
        let tmp = tempfile::tempdir().unwrap();
        let legacy_cert = tmp.path().join("cert.pem");
        let legacy_key = tmp.path().join("key.pem");
        let (cert, key) = crate::identity::cert::generate_self_signed().unwrap();
        std::fs::write(&legacy_cert, &cert).unwrap();
        std::fs::write(&legacy_key, &key).unwrap();

        let id = Identity::load_or_create(tmp.path(), Some("legacy".into())).unwrap();
        assert!(tmp.path().join("identity/cert.pem").exists());
        assert!(!legacy_cert.exists());
        assert_eq!(id.cert_pem.trim(), cert.trim());
    }
}
