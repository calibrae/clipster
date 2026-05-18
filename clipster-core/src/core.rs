use crate::db::Database;
use crate::identity::Identity;
use crate::peer::TrustManager;
use crate::sync::SyncEngine;
use std::path::PathBuf;
use std::sync::Arc;

/// Root handle to all Clipster subsystems. Shared between server and Tauri app.
#[derive(Clone)]
pub struct ClipsterCore {
    pub db: Arc<Database>,
    pub identity: Arc<Identity>,
    pub image_dir: PathBuf,
    pub trust: Arc<TrustManager>,
}

impl ClipsterCore {
    pub fn new(
        db: Database,
        identity: Identity,
        image_dir: PathBuf,
    ) -> Self {
        let db = Arc::new(db);
        let trust = Arc::new(TrustManager::new(db.clone()));
        Self {
            db,
            identity: Arc::new(identity),
            image_dir,
            trust,
        }
    }

    /// Spawn the background SyncEngine. Returns the handle for tests.
    pub fn spawn_sync_engine(self: &Arc<Self>) -> SyncEngine {
        SyncEngine::spawn(self.clone())
    }
}
