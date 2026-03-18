use clipster_common::config::ClientConfig;
use clipster_common::models::{Clip, ClipListResponse};
use uuid::Uuid;

pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl ApiClient {
    pub fn new(config: &ClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.insecure)
            .build()
            .expect("failed to build HTTP client");

        Self {
            http,
            base_url: format!("{}/api/v1", config.server_url.trim_end_matches('/')),
            api_key: config.api_key.clone(),
        }
    }

    pub async fn list(
        &self,
        limit: u32,
        content_type: Option<&str>,
        device: Option<&str>,
    ) -> anyhow::Result<ClipListResponse> {
        let mut url = format!("{}/clips?limit={limit}", self.base_url);
        if let Some(ct) = content_type {
            url.push_str(&format!("&type={ct}"));
        }
        if let Some(d) = device {
            url.push_str(&format!("&device={d}"));
        }
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn get(&self, id: &Uuid) -> anyhow::Result<Clip> {
        let resp = self
            .http
            .get(format!("{}/clips/{id}", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }

    pub async fn get_content(&self, id: &Uuid) -> anyhow::Result<Vec<u8>> {
        let resp = self
            .http
            .get(format!("{}/clips/{id}/content", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(resp.to_vec())
    }

    pub async fn search(&self, query: &str, limit: u32) -> anyhow::Result<ClipListResponse> {
        let url = format!(
            "{}/clips?search={}&limit={limit}",
            self.base_url,
            urlencoding::encode(query)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }
}
