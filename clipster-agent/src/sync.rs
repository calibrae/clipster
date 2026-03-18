use clipster_common::config::ClientConfig;
use clipster_common::models::CreateTextClipRequest;

pub struct SyncClient {
    http: reqwest::Client,
    server_url: String,
    api_key: String,
    device_name: String,
}

impl SyncClient {
    pub fn new(config: &ClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.insecure)
            .build()
            .expect("failed to build HTTP client");

        Self {
            http,
            server_url: config.server_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            device_name: config.device_name.clone(),
        }
    }

    pub async fn push_text(&self, text: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v1/clips", self.server_url);
        let req = CreateTextClipRequest {
            text_content: text.to_string(),
            source_device: self.device_name.clone(),
            source_app: None,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            tracing::debug!("duplicate clip, skipping");
            return Ok(());
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("server returned {status}: {body}");
        }

        tracing::info!("text clip synced");
        Ok(())
    }

    pub async fn push_image(
        &self,
        rgba_data: &[u8],
        width: usize,
        height: usize,
    ) -> anyhow::Result<()> {
        // Encode RGBA data to PNG
        let png_data = encode_rgba_to_png(rgba_data, width, height)?;

        let url = format!("{}/api/v1/clips", self.server_url);

        let metadata = serde_json::json!({
            "source_device": self.device_name,
            "image_mime": "image/png",
        });

        let form = reqwest::multipart::Form::new()
            .text("metadata", metadata.to_string())
            .part(
                "image",
                reqwest::multipart::Part::bytes(png_data)
                    .file_name("clipboard.png")
                    .mime_str("image/png")?,
            );

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            tracing::debug!("duplicate image clip, skipping");
            return Ok(());
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("server returned {status}: {body}");
        }

        tracing::info!("image clip synced");
        Ok(())
    }
}

fn encode_rgba_to_png(rgba: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<u8>> {
    use std::io::Cursor;
    let mut buf = Cursor::new(Vec::new());
    let mut encoder = png::Encoder::new(&mut buf, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(buf.into_inner())
}
