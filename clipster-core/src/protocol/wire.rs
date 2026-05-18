use chrono::{DateTime, Utc};
use clipster_common::models::Clip;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloRequest {
    pub device_id: String,
    pub name: String,
    pub version: String,
    pub proto: u32,
    pub caps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloResponse {
    pub device_id: String,
    pub name: String,
    pub server_time: DateTime<Utc>,
}

/// Re-export Clip under the wire-types namespace for clarity.
pub type PeerClip = Clip;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipPage {
    pub clips: Vec<PeerClip>,
    pub next_since: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipsSinceQuery {
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}
