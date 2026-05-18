//! Local API dispatch.
//!
//! Replaces the old reqwest-proxy `api_request` Tauri command. The web UI
//! still calls `invoke('api_request', { req: { method, path, body } })` and
//! `invoke('api_fetch_bytes', { path })` — but now the request is routed
//! directly to the local ClipsterCore (no HTTP) instead of a remote server.

use crate::core;
use base64::Engine;
use chrono::Utc;
use clipster_common::models::{Clip, ClipContentType, ClipListQuery, ClipListResponse, content_hash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ApiRequest {
    method: String,
    path: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse {
    pub status: u16,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

impl ApiResponse {
    fn json(status: u16, value: impl Serialize) -> Self {
        Self {
            status,
            body: serde_json::to_string(&value).unwrap_or_default(),
            content_type: Some("application/json".into()),
        }
    }
    fn empty(status: u16) -> Self {
        Self {
            status,
            body: String::new(),
            content_type: None,
        }
    }
    fn err(status: u16, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": message }))
    }
}

#[tauri::command]
pub async fn api_request(req: ApiRequest) -> Result<ApiResponse, String> {
    let method = req.method.to_uppercase();
    let (path, query) = split_query(&req.path);

    let core = core();
    let res: Result<ApiResponse, String> = match (method.as_str(), path.as_str()) {
        ("GET", "/clips") => list_clips(&core, &query).await,
        ("DELETE", "/clips") => delete_all(&core, &query).await,
        ("POST", "/clips") => create_text_clip(&core, req.body.as_deref()).await,
        ("GET", p) if p.starts_with("/clips/") && p.ends_with("/content") => {
            match id_from(p, "/clips/", "/content") {
                Ok(id) => get_clip_content(&core, &id).await,
                Err(r) => Ok(r),
            }
        }
        ("PATCH", p) if p.starts_with("/clips/") && p.ends_with("/favorite") => {
            match id_from(p, "/clips/", "/favorite") {
                Ok(id) => toggle_favorite(&core, &id).await,
                Err(r) => Ok(r),
            }
        }
        ("GET", p) if p.starts_with("/clips/") => match id_from(p, "/clips/", "") {
            Ok(id) => get_clip(&core, &id).await,
            Err(r) => Ok(r),
        },
        ("DELETE", p) if p.starts_with("/clips/") => match id_from(p, "/clips/", "") {
            Ok(id) => delete_clip(&core, &id).await,
            Err(r) => Ok(r),
        },
        ("GET", "/health") => Ok(ApiResponse::json(200, serde_json::json!({ "status": "ok" }))),
        _ => Ok(ApiResponse::err(
            404,
            &format!("no local route for {method} {path}"),
        )),
    };

    Ok(res.unwrap_or_else(|e| ApiResponse::err(500, &e)))
}

#[tauri::command]
pub async fn api_fetch_bytes(path: String) -> Result<String, String> {
    let core = core();
    let (p, _) = split_query(&path);
    if p.starts_with("/clips/") && p.ends_with("/content") {
        let id = id_from(&p, "/clips/", "/content").map_err(|r| r.body)?;
        let id: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
        let clip = core.db.get_clip(&id).map_err(|e| e.to_string())?;
        match clip.content_type {
            ClipContentType::Text => {
                let text = clip.text_content.unwrap_or_default();
                Ok(base64::engine::general_purpose::STANDARD.encode(text.as_bytes()))
            }
            ClipContentType::Image => {
                let hash = clip.image_hash.ok_or("missing image hash")?;
                let bytes = read_blob(&core, &hash, clip.image_mime.as_deref())
                    .await
                    .ok_or("blob not found")?;
                Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
            }
            _ => Err("unsupported content type".into()),
        }
    } else {
        Err(format!("no fetch route for {p}"))
    }
}

// ── Handlers ────────────────────────────────────────────────────────────

async fn list_clips(
    core: &clipster_core::ClipsterCore,
    query: &HashMap<String, String>,
) -> Result<ApiResponse, String> {
    let q = ClipListQuery {
        limit: query.get("limit").and_then(|s| s.parse().ok()),
        offset: query.get("offset").and_then(|s| s.parse().ok()),
        content_type: query.get("content_type").or_else(|| query.get("type")).cloned(),
        search: query.get("search").cloned(),
        device: query.get("device").cloned(),
        since: query.get("since").and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc)),
        exclude_device: query.get("exclude_device").cloned(),
    };
    let (clips, total_count) = core.db.list_clips(&q).map_err(|e| e.to_string())?;
    Ok(ApiResponse::json(200, ClipListResponse { clips, total_count }))
}

async fn delete_all(
    core: &clipster_core::ClipsterCore,
    query: &HashMap<String, String>,
) -> Result<ApiResponse, String> {
    let keep = query
        .get("keep_favorites")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let deleted = core.db.delete_all(keep).map_err(|e| e.to_string())?;
    Ok(ApiResponse::json(200, serde_json::json!({ "deleted": deleted })))
}

async fn create_text_clip(
    core: &clipster_core::ClipsterCore,
    body: Option<&str>,
) -> Result<ApiResponse, String> {
    #[derive(Deserialize)]
    struct TextReq {
        text_content: String,
        #[serde(default)]
        source_device: Option<String>,
        #[serde(default)]
        source_app: Option<String>,
    }

    let body = body.ok_or("missing body")?;
    let req: TextReq = serde_json::from_str(body).map_err(|e| e.to_string())?;

    let hash = content_hash(req.text_content.as_bytes());
    if core.db.has_recent_duplicate(&hash, 5).map_err(|e| e.to_string())? {
        return Ok(ApiResponse::err(409, "duplicate content"));
    }
    let now = Utc::now();
    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Text,
        text_content: Some(req.text_content.clone()),
        image_hash: None,
        image_mime: None,
        file_ref_path: None,
        content_hash: hash,
        source_device: req.source_device.unwrap_or_else(|| core.identity.device_name.clone()),
        source_app: req.source_app,
        byte_size: req.text_content.len() as u64,
        created_at: now,
        state_modified_at: now,
        is_favorite: false,
        is_deleted: false,
    };
    core.db.insert_clip(&clip).map_err(|e| e.to_string())?;
    Ok(ApiResponse::json(201, &clip))
}

async fn get_clip(
    core: &clipster_core::ClipsterCore,
    id_str: &str,
) -> Result<ApiResponse, String> {
    let id: Uuid = id_str.parse().map_err(|e: uuid::Error| e.to_string())?;
    match core.db.get_clip(&id) {
        Ok(c) => Ok(ApiResponse::json(200, &c)),
        Err(_) => Ok(ApiResponse::err(404, "clip not found")),
    }
}

async fn get_clip_content(
    core: &clipster_core::ClipsterCore,
    id_str: &str,
) -> Result<ApiResponse, String> {
    let id: Uuid = id_str.parse().map_err(|e: uuid::Error| e.to_string())?;
    let clip = match core.db.get_clip(&id) {
        Ok(c) => c,
        Err(_) => return Ok(ApiResponse::err(404, "clip not found")),
    };
    match clip.content_type {
        ClipContentType::Text => Ok(ApiResponse {
            status: 200,
            body: clip.text_content.unwrap_or_default(),
            content_type: Some("text/plain; charset=utf-8".into()),
        }),
        ClipContentType::Image => Ok(ApiResponse {
            status: 200,
            body: String::new(),
            content_type: clip.image_mime.clone(),
        }),
        _ => Ok(ApiResponse::err(400, "unsupported content type")),
    }
}

async fn delete_clip(
    core: &clipster_core::ClipsterCore,
    id_str: &str,
) -> Result<ApiResponse, String> {
    let id: Uuid = id_str.parse().map_err(|e: uuid::Error| e.to_string())?;
    match core.db.soft_delete(&id) {
        Ok(_) => Ok(ApiResponse::empty(204)),
        Err(_) => Ok(ApiResponse::err(404, "clip not found")),
    }
}

async fn toggle_favorite(
    core: &clipster_core::ClipsterCore,
    id_str: &str,
) -> Result<ApiResponse, String> {
    let id: Uuid = id_str.parse().map_err(|e: uuid::Error| e.to_string())?;
    let fav = core.db.toggle_favorite(&id).map_err(|e| e.to_string())?;
    Ok(ApiResponse::json(200, serde_json::json!({ "is_favorite": fav })))
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn read_blob(
    core: &clipster_core::ClipsterCore,
    hash: &str,
    mime: Option<&str>,
) -> Option<Vec<u8>> {
    let ext = mime_to_ext(mime.unwrap_or("image/png"));
    let path = core.image_dir.join(format!("{hash}.{ext}"));
    if let Ok(bytes) = tokio::fs::read(&path).await {
        return Some(bytes);
    }
    for try_ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp", "bin"] {
        let p = core.image_dir.join(format!("{hash}.{try_ext}"));
        if let Ok(bytes) = tokio::fs::read(&p).await {
            return Some(bytes);
        }
    }
    None
}

fn split_query(path: &str) -> (String, HashMap<String, String>) {
    let mut q = HashMap::new();
    let (p, qs) = match path.find('?') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => (path, ""),
    };
    for pair in qs.split('&').filter(|s| !s.is_empty()) {
        if let Some((k, v)) = pair.split_once('=') {
            q.insert(
                urlencoding::decode(k).map(|c| c.into_owned()).unwrap_or_else(|_| k.into()),
                urlencoding::decode(v).map(|c| c.into_owned()).unwrap_or_else(|_| v.into()),
            );
        } else {
            q.insert(pair.to_string(), String::new());
        }
    }
    (p.to_string(), q)
}

fn id_from(path: &str, prefix: &str, suffix: &str) -> Result<String, ApiResponse> {
    if !path.starts_with(prefix) {
        return Err(ApiResponse::err(404, "bad path"));
    }
    let rest = &path[prefix.len()..];
    let id = if suffix.is_empty() {
        rest.to_string()
    } else if let Some(s) = rest.strip_suffix(suffix) {
        s.to_string()
    } else {
        return Err(ApiResponse::err(404, "bad path"));
    };
    Ok(id)
}

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    }
}
