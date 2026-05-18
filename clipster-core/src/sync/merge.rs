use crate::ClipsterCore;
use crate::peer::PeerClient;
use clipster_common::error::ClipsterError;
use clipster_common::models::{Clip, ClipContentType};
use std::sync::Arc;

/// Merge a single remote clip into the local DB, fetching the image blob
/// if needed.
pub async fn merge_remote_clip(
    core: &Arc<ClipsterCore>,
    client: &PeerClient,
    remote: Clip,
) -> Result<(), ClipsterError> {
    let need_blob = matches!(remote.content_type, ClipContentType::Image)
        && remote.image_hash.is_some()
        && !remote.is_deleted;

    let image_hash = remote.image_hash.clone();
    let image_mime = remote.image_mime.clone();

    core.db.upsert_clip_from_peer(&remote)?;

    if need_blob {
        if let Some(hash) = image_hash {
            if let Err(e) = fetch_blob_if_missing(core, client, &hash, image_mime.as_deref()).await
            {
                tracing::warn!(error = %e, hash, "failed to fetch image blob");
            }
        }
    }

    Ok(())
}

async fn fetch_blob_if_missing(
    core: &ClipsterCore,
    client: &PeerClient,
    hash: &str,
    mime: Option<&str>,
) -> anyhow::Result<()> {
    let ext = mime_to_ext(mime.unwrap_or("image/png"));
    let path = core.image_dir.join(format!("{hash}.{ext}"));
    if path.exists() {
        return Ok(());
    }

    let bytes = client.fetch_blob(hash).await?;
    tokio::fs::create_dir_all(&core.image_dir).await.ok();
    tokio::fs::write(&path, &bytes).await?;
    tracing::debug!(hash, bytes = bytes.len(), "fetched image blob from peer");
    Ok(())
}

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    }
}
