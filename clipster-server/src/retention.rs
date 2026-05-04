use crate::db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1 hour

pub fn spawn(db: Arc<Database>, image_dir: PathBuf, retention_days: u32) {
    if retention_days == 0 {
        tracing::info!("retention disabled (retention_days = 0)");
        return;
    }

    tracing::info!(retention_days, "retention sweeper starting");

    tokio::spawn(async move {
        // Run an initial sweep shortly after startup, then on the interval.
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if let Err(e) = sweep(&db, &image_dir, retention_days).await {
                tracing::error!(error = %e, "retention sweep failed");
            }
            tokio::time::sleep(SWEEP_INTERVAL).await;
        }
    });
}

async fn sweep(
    db: &Database,
    image_dir: &std::path::Path,
    retention_days: u32,
) -> anyhow::Result<()> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let (deleted, orphan_hashes) = db.purge_older_than(cutoff)?;

    if deleted > 0 {
        tracing::info!(deleted, orphans = orphan_hashes.len(), "retention sweep");
    }

    // Best-effort image file cleanup.
    for hash in orphan_hashes {
        for ext in ["png", "jpg", "gif", "webp", "bmp", "bin"] {
            let p = image_dir.join(format!("{hash}.{ext}"));
            if p.exists() {
                if let Err(e) = tokio::fs::remove_file(&p).await {
                    tracing::warn!(path = %p.display(), error = %e, "failed to remove orphan image");
                }
            }
        }
    }

    Ok(())
}
