use crate::ClipsterCore;
use crate::discovery::PeerEvent;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;

/// SyncEngine spawns per-peer pull loops. The engine itself is driven by:
/// - mDNS discovery events (peer appears → maybe start a puller)
/// - trust changes (peer becomes trusted → start; revoked → stop)
pub struct SyncEngine {
    handles: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
    core: Arc<ClipsterCore>,
}

impl SyncEngine {
    pub fn spawn(core: Arc<ClipsterCore>) -> Self {
        let engine = Self {
            handles: Arc::new(Mutex::new(HashMap::new())),
            core: core.clone(),
        };

        // Start pullers for all already-trusted peers immediately.
        if let Ok(trusted) = core.db.list_trusted_peers() {
            for p in trusted {
                engine.ensure_puller(&p.device_id);
            }
        }

        // Listen for trust changes.
        let mut trust_rx = core.trust.trust_changes();
        let handles = engine.handles.clone();
        let core_ref = core.clone();
        let h2 = handles.clone();
        tokio::spawn(async move {
            while trust_rx.changed().await.is_ok() {
                let trusted = match core_ref.db.list_trusted_peers() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let trusted_ids: std::collections::HashSet<String> =
                    trusted.iter().map(|p| p.device_id.clone()).collect();

                // Start new ones
                for p in &trusted {
                    let mut guard = h2.lock().unwrap();
                    if !guard.contains_key(&p.device_id) {
                        let c = core_ref.clone();
                        let id = p.device_id.clone();
                        let handle = tokio::spawn(super::puller::pull_loop(c, id.clone()));
                        guard.insert(id, handle);
                    }
                }

                // Stop ones no longer trusted
                let to_stop: Vec<String> = {
                    let guard = h2.lock().unwrap();
                    guard.keys().filter(|k| !trusted_ids.contains(*k)).cloned().collect()
                };
                let mut guard = h2.lock().unwrap();
                for id in to_stop {
                    if let Some(h) = guard.remove(&id) {
                        h.abort();
                    }
                }
            }
        });

        engine
    }

    /// Subscribe to discovery events (called by the binary mounting this engine).
    pub fn attach_discovery(&self, mut rx: Receiver<PeerEvent>) {
        let core = self.core.clone();
        tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                match event {
                    PeerEvent::Discovered(peer) => {
                        let addr_str = peer
                            .addrs
                            .first()
                            .map(|a| a.to_string())
                            .unwrap_or_default();
                        let caps = serde_json::to_string(&peer.capabilities).ok();
                        if let Err(e) = core.trust.on_discovered(
                            &peer.device_id,
                            &peer.name,
                            &addr_str,
                            caps.as_deref(),
                        ) {
                            tracing::warn!(error = %e, "trust.on_discovered");
                        }
                    }
                    PeerEvent::Lost { fullname } => {
                        tracing::debug!(%fullname, "peer lost");
                    }
                }
            }
        });
    }

    fn ensure_puller(&self, peer_id: &str) {
        let mut guard = self.handles.lock().unwrap();
        if guard.contains_key(peer_id) {
            return;
        }
        let core = self.core.clone();
        let id = peer_id.to_string();
        let handle = tokio::spawn(super::puller::pull_loop(core, id.clone()));
        guard.insert(id, handle);
    }
}
