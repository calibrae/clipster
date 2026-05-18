use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustStatus {
    Pending,
    Trusted,
    Rejected,
}

impl std::fmt::Display for TrustStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Trusted => write!(f, "trusted"),
            Self::Rejected => write!(f, "rejected"),
        }
    }
}

impl std::str::FromStr for TrustStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "trusted" => Ok(Self::Trusted),
            "rejected" => Ok(Self::Rejected),
            other => Err(format!("unknown trust status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub device_id: String,
    pub name: String,
    pub trust_status: TrustStatus,
    pub pinned_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_addr: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub capabilities: Option<String>,
}
