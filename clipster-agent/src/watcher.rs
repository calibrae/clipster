use crate::sync::SyncClient;
use clipster_common::models::content_hash;
use std::time::Duration;

pub async fn run(client: SyncClient) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    let mut last_text_hash: Option<String> = None;
    let mut last_image_hash: Option<String> = None;

    tracing::info!("clipboard watcher started, polling every 500ms");

    loop {
        // Check text
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                let hash = content_hash(text.as_bytes());
                if last_text_hash.as_ref() != Some(&hash) {
                    last_text_hash = Some(hash);
                    tracing::debug!(len = text.len(), "new text clip detected");
                    if let Err(e) = client.push_text(&text).await {
                        tracing::error!(error = %e, "failed to push text clip");
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
                tracing::debug!(
                    width = img.width,
                    height = img.height,
                    bytes = raw.len(),
                    "new image clip detected"
                );
                if let Err(e) = client.push_image(raw, img.width, img.height).await {
                    tracing::error!(error = %e, "failed to push image clip");
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
