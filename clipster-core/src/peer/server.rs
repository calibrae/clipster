use crate::ClipsterCore;
use crate::protocol::wire::{ClipPage, HelloRequest, HelloResponse};
use axum::{
    Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;

/// Build the peer-facing sub-router.
/// Mount this at root on a TLS listener; routes are prefixed `/api/v1/peer/...`.
pub fn router(core: Arc<ClipsterCore>) -> Router {
    Router::new()
        .route("/api/v1/peer/hello", post(hello))
        .route("/api/v1/peer/clips", get(list_clips))
        .route("/api/v1/peer/clips/{id}/content", get(get_clip_content))
        .route("/api/v1/peer/blobs/{hash}", get(get_blob))
        .route_layer(middleware::from_fn_with_state(
            core.clone(),
            peer_auth_middleware,
        ))
        .with_state(core)
}

async fn peer_auth_middleware(
    State(core): State<Arc<ClipsterCore>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let device_id = req
        .headers()
        .get("X-Clipster-Device")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let Some(device_id) = device_id else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    if !core.trust.is_trusted(&device_id) {
        tracing::debug!(device_id, "peer auth rejected (not trusted)");
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(req).await)
}

async fn hello(
    State(core): State<Arc<ClipsterCore>>,
    Json(_req): Json<HelloRequest>,
) -> Json<HelloResponse> {
    Json(HelloResponse {
        device_id: core.identity.device_id.clone(),
        name: core.identity.device_name.clone(),
        server_time: Utc::now(),
    })
}

#[derive(Deserialize)]
struct ClipsSinceParams {
    #[serde(default)]
    since: Option<DateTime<Utc>>,
    #[serde(default)]
    limit: Option<u32>,
}

async fn list_clips(
    State(core): State<Arc<ClipsterCore>>,
    Query(p): Query<ClipsSinceParams>,
) -> Result<Json<ClipPage>, StatusCode> {
    let since = p.since.unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap());
    let limit = p.limit.unwrap_or(500).min(1000);

    let now = Utc::now();
    let clips = core
        .db
        .list_clips_since(since, limit)
        .map_err(|e| {
            tracing::error!(error = %e, "peer list_clips_since");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // next_since: advance to latest state_modified_at on page, or now if page is short
    let next_since = clips.last().map(|c| c.state_modified_at).unwrap_or(now);

    Ok(Json(ClipPage { clips, next_since }))
}

async fn get_clip_content(
    State(core): State<Arc<ClipsterCore>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, StatusCode> {
    let clip = core
        .db
        .get_clip(&id)
        .map_err(|_| StatusCode::NOT_FOUND)?;

    use clipster_common::models::ClipContentType;
    match clip.content_type {
        ClipContentType::Text => {
            let text = clip.text_content.unwrap_or_default();
            Ok((
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                text,
            )
                .into_response())
        }
        ClipContentType::Image => {
            let Some(hash) = clip.image_hash else {
                return Err(StatusCode::NOT_FOUND);
            };
            blob_response(&core, &hash, clip.image_mime.as_deref()).await
        }
        ClipContentType::FileRef => Err(StatusCode::BAD_REQUEST),
    }
}

async fn get_blob(
    State(core): State<Arc<ClipsterCore>>,
    Path(hash): Path<String>,
) -> Result<Response, StatusCode> {
    blob_response(&core, &hash, None).await
}

async fn blob_response(
    core: &ClipsterCore,
    hash: &str,
    mime_hint: Option<&str>,
) -> Result<Response, StatusCode> {
    let mime = mime_hint.unwrap_or("application/octet-stream").to_string();
    let ext = mime_to_ext(&mime);
    let path = core.image_dir.join(format!("{hash}.{ext}"));

    match tokio::fs::read(&path).await {
        Ok(data) => Ok(([(header::CONTENT_TYPE, mime.as_str())], data).into_response()),
        Err(_) => {
            // Try all possible extensions (mime hint may be missing for /peer/blobs/:hash)
            for try_ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp", "bin"] {
                let p = core.image_dir.join(format!("{hash}.{try_ext}"));
                if let Ok(data) = tokio::fs::read(&p).await {
                    let m = ext_to_mime(try_ext);
                    return Ok(([(header::CONTENT_TYPE, m)], data).into_response());
                }
            }
            Err(StatusCode::NOT_FOUND)
        }
    }
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

fn ext_to_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

// HeaderMap import preserved for completeness (used in future custom auth helpers)
#[allow(dead_code)]
fn _suppress_header_warn(_h: HeaderMap) {}
