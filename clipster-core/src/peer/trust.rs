use crate::db::Database;
use crate::db::peers::PeerRecord;
pub use crate::db::peers::TrustStatus;
use clipster_common::error::ClipsterError;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone)]
pub struct PendingApprovalEvent {
    pub device_id: String,
    pub name: String,
    pub addr: String,
}

/// In-memory trust state, refreshed from the DB and broadcast to subscribers.
pub struct TrustManager {
    db: Arc<Database>,
    trusted_ids: RwLock<HashMap<String, PeerRecord>>,
    pending_tx: broadcast::Sender<PendingApprovalEvent>,
    trusted_watch_tx: watch::Sender<u64>, // bumped on every trust change
}

impl TrustManager {
    pub fn new(db: Arc<Database>) -> Self {
        let (pending_tx, _) = broadcast::channel(32);
        let (trusted_watch_tx, _) = watch::channel(0u64);
        let mgr = Self {
            db,
            trusted_ids: RwLock::new(HashMap::new()),
            pending_tx,
            trusted_watch_tx,
        };
        if let Err(e) = mgr.refresh_trusted() {
            tracing::warn!(error = %e, "failed to load trusted peers on init");
        }
        mgr
    }

    pub fn pending_events(&self) -> broadcast::Receiver<PendingApprovalEvent> {
        self.pending_tx.subscribe()
    }

    pub fn trust_changes(&self) -> watch::Receiver<u64> {
        self.trusted_watch_tx.subscribe()
    }

    /// Called by discovery: classify the peer and emit pending if needed.
    pub fn on_discovered(
        &self,
        device_id: &str,
        name: &str,
        addr: &str,
        capabilities: Option<&str>,
    ) -> Result<TrustStatus, ClipsterError> {
        let status = self
            .db
            .upsert_peer_discovery(device_id, name, addr, capabilities)?;
        match status {
            TrustStatus::Pending => {
                let _ = self.pending_tx.send(PendingApprovalEvent {
                    device_id: device_id.to_string(),
                    name: name.to_string(),
                    addr: addr.to_string(),
                });
                tracing::warn!(
                    device_id, name, addr, "pending peer discovered — awaiting trust"
                );
            }
            TrustStatus::Trusted => {
                self.refresh_trusted()?;
            }
            TrustStatus::Rejected => {
                tracing::debug!(device_id, "ignoring rejected peer");
            }
        }
        Ok(status)
    }

    pub fn trust(&self, device_id: &str) -> Result<(), ClipsterError> {
        self.db.set_peer_trust(device_id, TrustStatus::Trusted)?;
        self.refresh_trusted()?;
        Ok(())
    }

    pub fn reject(&self, device_id: &str) -> Result<(), ClipsterError> {
        self.db.set_peer_trust(device_id, TrustStatus::Rejected)?;
        self.refresh_trusted()?;
        Ok(())
    }

    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.trusted_ids
            .read()
            .map(|m| m.contains_key(device_id))
            .unwrap_or(false)
    }

    pub fn trusted_peers(&self) -> Vec<PeerRecord> {
        self.trusted_ids
            .read()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn refresh_trusted(&self) -> Result<(), ClipsterError> {
        let peers = self.db.list_trusted_peers()?;
        let map: HashMap<String, PeerRecord> =
            peers.into_iter().map(|p| (p.device_id.clone(), p)).collect();
        if let Ok(mut w) = self.trusted_ids.write() {
            *w = map;
        }
        // Bump watch tick
        self.trusted_watch_tx.send_modify(|v| *v = v.wrapping_add(1));
        Ok(())
    }
}
