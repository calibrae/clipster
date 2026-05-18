use clipster_core::ClipsterCore;
use clipster_core::db::Database;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<ClipsterCore>,
    pub db: Arc<Database>,
    pub image_dir: String,
    pub api_key: Option<String>,
}

impl AppState {
    pub fn new(core: Arc<ClipsterCore>, api_key: Option<String>) -> Self {
        let db = core.db.clone();
        let image_dir = core.image_dir.to_string_lossy().to_string();
        Self {
            core,
            db,
            image_dir,
            api_key,
        }
    }
}
