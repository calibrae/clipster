use clipster_common::models::{CreateTextClipRequest, content_hash};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{current_settings, build_http_client};

/// Main sync loop — watches clipboard and pushes changes to server.
/// Restarts when `restart_flag` is set (e.g. after settings change).
pub async fn run_sync_loop(restart_flag: Arc<AtomicBool>) {
    loop {
        let settings = current_settings();

        if !settings.sync_enabled || settings.server_url.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if restart_flag.swap(false, Ordering::Relaxed) {
                continue;
            }
            continue;
        }

        tracing::info!(server = %settings.server_url, "clipboard sync started");

        let client = build_http_client(&settings);
        let base_url = settings.server_url.trim_end_matches('/').to_string();
        let api_key = settings.api_key.clone();
        let device_name = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string());

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "failed to open clipboard");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut last_text_hash: Option<String> = None;
        let mut last_image_hash: Option<String> = None;

        loop {
            // Check if settings changed — break inner loop to restart
            if restart_flag.swap(false, Ordering::Relaxed) {
                tracing::info!("settings changed, restarting sync");
                break;
            }

            // Check text
            if let Ok(text) = clipboard.get_text() {
                if !text.is_empty() {
                    let hash = content_hash(text.as_bytes());
                    if last_text_hash.as_ref() != Some(&hash) {
                        last_text_hash = Some(hash);

                        let req = CreateTextClipRequest {
                            text_content: text,
                            source_device: device_name.clone(),
                            source_app: None,
                        };

                        let url = format!("{base_url}/api/v1/clips");
                        let mut builder = client.post(&url).json(&req);
                        if !api_key.is_empty() {
                            builder = builder.bearer_auth(&api_key);
                        }

                        match builder.send().await {
                            Ok(resp) if resp.status() == reqwest::StatusCode::CONFLICT => {
                                tracing::debug!("duplicate clip, skipping");
                            }
                            Ok(resp) if resp.status().is_success() => {
                                tracing::debug!("text clip synced");
                            }
                            Ok(resp) => {
                                tracing::warn!(status = %resp.status(), "failed to push text clip");
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "failed to push text clip");
                            }
                        }
                    }
                }
            }

            // Check image
            if let Ok(img) = clipboard.get_image() {
                let raw = img.bytes.as_ref();
                let hash = content_hash(raw);
                if last_image_hash.as_ref() != Some(&hash) {
                    last_image_hash = Some(hash);

                    match encode_rgba_to_png(raw, img.width, img.height) {
                        Ok(png_data) => {
                            let metadata = serde_json::json!({
                                "source_device": device_name,
                                "image_mime": "image/png",
                            });

                            let form = reqwest::multipart::Form::new()
                                .text("metadata", metadata.to_string())
                                .part(
                                    "image",
                                    reqwest::multipart::Part::bytes(png_data)
                                        .file_name("clipboard.png")
                                        .mime_str("image/png")
                                        .unwrap(),
                                );

                            let url = format!("{base_url}/api/v1/clips");
                            let mut builder = client.post(&url).multipart(form);
                            if !api_key.is_empty() {
                                builder = builder.bearer_auth(&api_key);
                            }

                            match builder.send().await {
                                Ok(resp) if resp.status() == reqwest::StatusCode::CONFLICT => {
                                    tracing::debug!("duplicate image, skipping");
                                }
                                Ok(resp) if resp.status().is_success() => {
                                    tracing::debug!("image clip synced");
                                }
                                Ok(resp) => {
                                    tracing::warn!(status = %resp.status(), "failed to push image clip");
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to push image clip");
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to encode image");
                        }
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
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
