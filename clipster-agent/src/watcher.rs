//! Clipboard watcher — polls the clipboard and inserts new clips into the
//! local core DB.

use chrono::Utc;
use clipster_common::models::{Clip, ClipContentType, content_hash};
use clipster_core::ClipsterCore;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub async fn run(core: Arc<ClipsterCore>) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    tracing::info!("clipboard watcher started, polling every 500ms");

    let device_name = core.identity.device_name.clone();
    let mut last_text: Option<String> = None;
    let mut last_image: Option<String> = None;

    loop {
        if let Ok(text) = clipboard.get_text() {
            if !text.is_empty() {
                let hash = content_hash(text.as_bytes());
                if last_text.as_ref() != Some(&hash) {
                    last_text = Some(hash);
                    if let Err(e) = insert_text(&core, text, &device_name).await {
                        tracing::warn!(error = %e, "insert text clip");
                    }
                }
            }
        }

        if let Ok(img) = clipboard.get_image() {
            let raw = img.bytes.as_ref();
            let hash = content_hash(raw);
            if last_image.as_ref() != Some(&hash) {
                last_image = Some(hash);
                if let Err(e) = insert_image(&core, raw, img.width, img.height, &device_name).await {
                    tracing::warn!(error = %e, "insert image clip");
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn insert_text(core: &ClipsterCore, text: String, device_name: &str) -> anyhow::Result<()> {
    let hash = content_hash(text.as_bytes());
    if core.db.has_recent_duplicate(&hash, 5)? {
        return Ok(());
    }
    let now = Utc::now();
    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Text,
        text_content: Some(text.clone()),
        image_hash: None,
        image_mime: None,
        file_ref_path: None,
        content_hash: hash,
        source_device: device_name.to_string(),
        source_app: None,
        byte_size: text.len() as u64,
        created_at: now,
        state_modified_at: now,
        is_favorite: false,
        is_deleted: false,
    };
    core.db.insert_clip(&clip)?;
    tracing::debug!(id = %clip.id, "captured text clip");
    Ok(())
}

async fn insert_image(
    core: &ClipsterCore,
    rgba: &[u8],
    width: usize,
    height: usize,
    device_name: &str,
) -> anyhow::Result<()> {
    let png_data = encode_rgba_to_png(rgba, width, height)?;
    let hash = content_hash(&png_data);
    if core.db.has_recent_duplicate(&hash, 5)? {
        return Ok(());
    }

    let path = core.image_dir.join(format!("{hash}.png"));
    tokio::fs::create_dir_all(&core.image_dir).await.ok();
    tokio::fs::write(&path, &png_data).await?;

    let now = Utc::now();
    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Image,
        text_content: None,
        image_hash: Some(hash.clone()),
        image_mime: Some("image/png".into()),
        file_ref_path: None,
        content_hash: hash,
        source_device: device_name.to_string(),
        source_app: None,
        byte_size: png_data.len() as u64,
        created_at: now,
        state_modified_at: now,
        is_favorite: false,
        is_deleted: false,
    };
    core.db.insert_clip(&clip)?;
    tracing::debug!(id = %clip.id, bytes = png_data.len(), "captured image clip");
    Ok(())
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
