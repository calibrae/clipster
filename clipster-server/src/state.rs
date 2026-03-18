use crate::db::Database;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub image_dir: String,
    pub api_key: Option<String>,
}

impl AppState {
    pub fn new(db: Database, image_dir: String, api_key: Option<String>) -> Self {
        Self {
            db: Arc::new(db),
            image_dir,
            api_key,
        }
    }
}
