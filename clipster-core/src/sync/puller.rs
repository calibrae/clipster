use super::merge::merge_remote_clip;
use crate::ClipsterCore;
use crate::db::peers::PeerRecord;
use crate::peer::PeerClient;
use chrono::{DateTime, Utc};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

const PAGE_SIZE: u32 = 500;
const POLL_IDLE: Duration = Duration::from_secs(5);
const POLL_BACKOFF: Duration = Duration::from_secs(15);

/// Per-peer pull loop. Runs until cancelled.
pub async fn pull_loop(core: Arc<ClipsterCore>, peer_id: String) {
    tracing::info!(peer = %peer_id, "puller started");
    loop {
        let peer = match core.db.get_peer(&peer_id) {
            Ok(Some(p)) => p,
            _ => {
                tracing::warn!(peer = %peer_id, "puller exiting: peer record gone");
                return;
            }
        };

        if peer.trust_status != crate::peer::TrustStatus::Trusted {
            tracing::info!(peer = %peer_id, "puller exiting: peer no longer trusted");
            return;
        }

        let Some(addr) = peer_socket_addr(&peer) else {
            tracing::debug!(peer = %peer_id, "no last_addr known yet, waiting");
            tokio::time::sleep(POLL_BACKOFF).await;
            continue;
        };

        let client = match PeerClient::new(addr, &peer.device_id, &core.identity) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(peer = %peer_id, error = %e, "failed to build peer client");
                tokio::time::sleep(POLL_BACKOFF).await;
                continue;
            }
        };

        match pull_once(&core, &client, &peer).await {
            Ok(count) => {
                if count < PAGE_SIZE as usize {
                    tokio::time::sleep(POLL_IDLE).await;
                }
                // else: continue immediately to catch up
            }
            Err(e) => {
                tracing::debug!(peer = %peer_id, error = %e, "pull failed");
                tokio::time::sleep(POLL_BACKOFF).await;
            }
        }
    }
}

async fn pull_once(
    core: &Arc<ClipsterCore>,
    client: &PeerClient,
    peer: &PeerRecord,
) -> anyhow::Result<usize> {
    let since = core
        .db
        .peer_last_sync(&peer.device_id)?
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());

    let page = client.get_clips_since(since, PAGE_SIZE).await?;
    let count = page.clips.len();

    for clip in page.clips {
        if let Err(e) = merge_remote_clip(core, client, clip).await {
            tracing::warn!(error = %e, "merge_remote_clip");
        }
    }

    core.db.set_peer_last_sync(&peer.device_id, page.next_since)?;
    Ok(count)
}

fn peer_socket_addr(peer: &PeerRecord) -> Option<SocketAddr> {
    peer.last_addr.as_deref().and_then(|s| SocketAddr::from_str(s).ok())
}
