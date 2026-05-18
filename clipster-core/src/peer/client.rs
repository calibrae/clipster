use crate::identity::Identity;
use crate::protocol::PinnedFingerprintVerifier;
use crate::protocol::wire::{ClipPage, HelloRequest, HelloResponse};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::net::SocketAddr;
use std::sync::Arc;

/// HTTP client for talking to another peer over pinned TLS.
pub struct PeerClient {
    http: reqwest::Client,
    base_url: String,
    own_device_id: String,
}

impl PeerClient {
    pub fn new(
        addr: SocketAddr,
        pinned_fp: &str,
        identity: &Identity,
    ) -> Result<Self> {
        // Ensure crypto provider installed
        let _ = rustls::crypto::ring::default_provider().install_default();

        let tls_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedFingerprintVerifier::new(
                pinned_fp,
            )))
            .with_no_client_auth();

        let http = reqwest::Client::builder()
            .use_preconfigured_tls(tls_config)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("building peer http client")?;

        Ok(Self {
            http,
            base_url: format!("https://{addr}"),
            own_device_id: identity.device_id.clone(),
        })
    }

    pub async fn hello(&self, req: &HelloRequest) -> Result<HelloResponse> {
        let resp = self
            .http
            .post(format!("{}/api/v1/peer/hello", self.base_url))
            .header("X-Clipster-Device", &self.own_device_id)
            .json(req)
            .send()
            .await
            .context("peer hello request")?;
        if !resp.status().is_success() {
            anyhow::bail!("hello returned {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn get_clips_since(
        &self,
        since: DateTime<Utc>,
        limit: u32,
    ) -> Result<ClipPage> {
        let resp = self
            .http
            .get(format!(
                "{}/api/v1/peer/clips?since={}&limit={}",
                self.base_url,
                urlencoding::encode(&since.to_rfc3339()),
                limit
            ))
            .header("X-Clipster-Device", &self.own_device_id)
            .send()
            .await
            .context("peer get_clips_since")?;
        if !resp.status().is_success() {
            anyhow::bail!("peer/clips returned {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn fetch_blob(&self, image_hash: &str) -> Result<Vec<u8>> {
        let resp = self
            .http
            .get(format!(
                "{}/api/v1/peer/blobs/{}",
                self.base_url, image_hash
            ))
            .header("X-Clipster-Device", &self.own_device_id)
            .send()
            .await
            .context("peer fetch_blob")?;
        if !resp.status().is_success() {
            anyhow::bail!("peer/blobs returned {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }
}
