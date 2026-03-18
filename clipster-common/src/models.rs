use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipContentType {
    Text,
    Image,
    FileRef,
}

impl std::fmt::Display for ClipContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Image => write!(f, "image"),
            Self::FileRef => write!(f, "file_ref"),
        }
    }
}

impl std::str::FromStr for ClipContentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "image" => Ok(Self::Image),
            "file_ref" => Ok(Self::FileRef),
            other => Err(format!("unknown content type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: Uuid,
    pub content_type: ClipContentType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_ref_path: Option<String>,
    pub content_hash: String,
    pub source_device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub byte_size: u64,
    pub created_at: DateTime<Utc>,
    pub is_favorite: bool,
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTextClipRequest {
    pub text_content: String,
    pub source_device: String,
    #[serde(default)]
    pub source_app: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageClipMetadata {
    pub source_device: String,
    pub image_mime: String,
    #[serde(default)]
    pub source_app: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClipListResponse {
    pub clips: Vec<Clip>,
    pub total_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct ClipListQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    #[serde(rename = "type")]
    pub content_type: Option<String>,
    pub search: Option<String>,
    pub device: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub exclude_device: Option<String>,
}

pub fn content_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn content_hash_consistent_for_same_input() {
        let data = b"hello clipboard";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
        // SHA-256 output is 64 hex chars
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        let h1 = content_hash(b"alpha");
        let h2 = content_hash(b"beta");
        assert_ne!(h1, h2);
    }

    #[test]
    fn clip_content_type_display_from_str_round_trip() {
        let variants = [
            (ClipContentType::Text, "text"),
            (ClipContentType::Image, "image"),
            (ClipContentType::FileRef, "file_ref"),
        ];
        for (variant, expected_str) in &variants {
            let displayed = variant.to_string();
            assert_eq!(&displayed, expected_str);
            let parsed: ClipContentType = displayed.parse().unwrap();
            assert_eq!(&parsed, variant);
        }
    }

    #[test]
    fn clip_content_type_from_str_unknown_returns_err() {
        let result = ClipContentType::from_str("video");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("unknown content type"));
    }

    #[test]
    fn clip_serialization_deserialization_round_trip() {
        let clip = Clip {
            id: Uuid::now_v7(),
            content_type: ClipContentType::Text,
            text_content: Some("test content".to_string()),
            image_hash: None,
            image_mime: None,
            file_ref_path: None,
            content_hash: content_hash(b"test content"),
            source_device: "device-1".to_string(),
            source_app: Some("terminal".to_string()),
            byte_size: 12,
            created_at: Utc::now(),
            is_favorite: false,
            is_deleted: false,
        };

        let json = serde_json::to_string(&clip).unwrap();
        let deserialized: Clip = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, clip.id);
        assert_eq!(deserialized.content_type, clip.content_type);
        assert_eq!(deserialized.text_content, clip.text_content);
        assert_eq!(deserialized.content_hash, clip.content_hash);
        assert_eq!(deserialized.source_device, clip.source_device);
        assert_eq!(deserialized.source_app, clip.source_app);
        assert_eq!(deserialized.byte_size, clip.byte_size);
        assert_eq!(deserialized.is_favorite, clip.is_favorite);
        assert_eq!(deserialized.is_deleted, clip.is_deleted);
    }

    #[test]
    fn create_text_clip_request_deserialization_with_source_app() {
        let json = r#"{"text_content":"hello","source_device":"mac","source_app":"vim"}"#;
        let req: CreateTextClipRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text_content, "hello");
        assert_eq!(req.source_device, "mac");
        assert_eq!(req.source_app, Some("vim".to_string()));
    }

    #[test]
    fn create_text_clip_request_deserialization_without_source_app() {
        let json = r#"{"text_content":"hello","source_device":"mac"}"#;
        let req: CreateTextClipRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text_content, "hello");
        assert_eq!(req.source_device, "mac");
        assert_eq!(req.source_app, None);
    }
}
