use axum::{
    Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, patch, post},
};
use clipster_common::models::{
    Clip, ClipContentType, ClipListQuery, ClipListResponse, CreateTextClipRequest,
    ImageClipMetadata, content_hash,
};
use rust_embed::Embed;
use subtle::ConstantTimeEq;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

use crate::state::AppState;

const MAX_BODY_SIZE: usize = 50 * 1024 * 1024; // 50 MB

#[derive(Embed)]
#[folder = "../web/"]
struct WebAssets;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/v1/clips", post(create_clip))
        .route("/api/v1/clips", get(list_clips))
        .route("/api/v1/clips/{id}", get(get_clip))
        .route("/api/v1/clips/{id}", delete(delete_clip))
        .route("/api/v1/clips/{id}/content", get(get_clip_content))
        .route("/api/v1/clips/{id}/favorite", patch(toggle_favorite))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(api)
        .route("/api/v1/health", get(health))
        .fallback(get(static_handler))
        .layer(middleware::from_fn(security_headers))
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(cors)
        .with_state(state)
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    resp
}

async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let Some(ref expected_key) = state.api_key else {
        // No API key configured — allow all requests
        return Ok(next.run(req).await);
    };

    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match provided {
        Some(token) => {
            let token_bytes = token.as_bytes();
            let expected_bytes = expected_key.as_bytes();
            // Constant-time comparison to prevent timing attacks
            if token_bytes.len() == expected_bytes.len()
                && token_bytes.ct_eq(expected_bytes).into()
            {
                Ok(next.run(req).await)
            } else {
                tracing::warn!("rejected request: invalid API key");
                Err(AppError::unauthorized())
            }
        }
        None => {
            tracing::warn!("rejected request: missing Authorization header");
            Err(AppError::unauthorized())
        }
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn create_clip(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let content_type_header = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    if content_type_header.starts_with("multipart/form-data") {
        return create_image_clip(state, headers, body).await;
    }

    let req: CreateTextClipRequest =
        serde_json::from_slice(&body).map_err(|e| AppError::bad_request(e.to_string()))?;

    let hash = content_hash(req.text_content.as_bytes());

    if state.db.has_recent_duplicate(&hash, 5)? {
        return Err(AppError::duplicate());
    }

    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Text,
        text_content: Some(req.text_content.clone()),
        image_hash: None,
        image_mime: None,
        file_ref_path: None,
        content_hash: hash,
        source_device: req.source_device,
        source_app: req.source_app,
        byte_size: req.text_content.len() as u64,
        created_at: chrono::Utc::now(),
        is_favorite: false,
        is_deleted: false,
    };

    state.db.insert_clip(&clip)?;
    tracing::info!(id = %clip.id, "created text clip");

    Ok((StatusCode::CREATED, Json(clip)).into_response())
}

async fn create_image_clip(
    state: AppState,
    _headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let boundary = _headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| multer::parse_boundary(ct).ok())
        .ok_or_else(|| AppError::bad_request("missing multipart boundary".into()))?;

    let mut multipart =
        multer::Multipart::new(futures_util::stream::once(async move { Ok::<_, std::io::Error>(body) }), boundary);

    let mut metadata: Option<ImageClipMetadata> = None;
    let mut image_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::bad_request(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "metadata" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AppError::bad_request(e.to_string()))?;
                metadata = Some(
                    serde_json::from_str(&text)
                        .map_err(|e| AppError::bad_request(e.to_string()))?,
                );
            }
            "image" => {
                image_data = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::bad_request(e.to_string()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let meta = metadata.ok_or_else(|| AppError::bad_request("missing metadata field".into()))?;
    let data = image_data.ok_or_else(|| AppError::bad_request("missing image field".into()))?;

    let hash = content_hash(&data);

    if state.db.has_recent_duplicate(&hash, 5)? {
        return Err(AppError::duplicate());
    }

    let ext = match meta.image_mime.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "bin",
    };
    let filename = format!("{hash}.{ext}");
    let path = std::path::Path::new(&state.image_dir).join(&filename);
    tokio::fs::write(&path, &data).await?;

    let clip = Clip {
        id: Uuid::now_v7(),
        content_type: ClipContentType::Image,
        text_content: None,
        image_hash: Some(hash.clone()),
        image_mime: Some(meta.image_mime),
        file_ref_path: None,
        content_hash: hash,
        source_device: meta.source_device,
        source_app: meta.source_app,
        byte_size: data.len() as u64,
        created_at: chrono::Utc::now(),
        is_favorite: false,
        is_deleted: false,
    };

    state.db.insert_clip(&clip)?;
    tracing::info!(id = %clip.id, "created image clip");

    Ok((StatusCode::CREATED, Json(clip)).into_response())
}

async fn list_clips(
    State(state): State<AppState>,
    Query(query): Query<ClipListQuery>,
) -> Result<Json<ClipListResponse>, AppError> {
    let (clips, total_count) = state.db.list_clips(&query)?;
    Ok(Json(ClipListResponse { clips, total_count }))
}

async fn get_clip(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Clip>, AppError> {
    let clip = state.db.get_clip(&id)?;
    Ok(Json(clip))
}

async fn get_clip_content(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let clip = state.db.get_clip(&id)?;
    match clip.content_type {
        ClipContentType::Text => {
            let text = clip.text_content.unwrap_or_default();
            Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response())
        }
        ClipContentType::Image => {
            let hash = clip
                .image_hash
                .ok_or_else(|| AppError::not_found("image not found".into()))?;
            let mime = clip.image_mime.unwrap_or("image/png".into());
            let ext = match mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/gif" => "gif",
                "image/webp" => "webp",
                "image/bmp" => "bmp",
                _ => "bin",
            };
            let path = std::path::Path::new(&state.image_dir).join(format!("{hash}.{ext}"));
            let data = tokio::fs::read(&path).await?;
            Ok(([(header::CONTENT_TYPE, mime.as_str())], data).into_response())
        }
        ClipContentType::FileRef => Err(AppError::bad_request("file refs not yet supported".into())),
    }
}

async fn delete_clip(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.db.soft_delete(&id)?;
    tracing::info!(id = %id, "deleted clip");
    Ok(StatusCode::NO_CONTENT)
}

async fn toggle_favorite(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let is_fav = state.db.toggle_favorite(&id)?;
    tracing::info!(id = %id, is_favorite = is_fav, "toggled favorite");
    Ok(Json(serde_json::json!({ "is_favorite": is_fav })))
}

async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => {
            match WebAssets::get("index.html") {
                Some(content) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html")],
                    content.data.into_owned(),
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }
}

// Error handling
struct AppError {
    status: StatusCode,
    message: String,
    internal: Option<String>,
}

impl AppError {
    fn bad_request(msg: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: msg,
            internal: None,
        }
    }
    fn not_found(msg: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: msg,
            internal: None,
        }
    }
    fn duplicate() -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: "duplicate content".into(),
            internal: None,
        }
    }
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "unauthorized".into(),
            internal: None,
        }
    }
}

impl From<clipster_common::error::ClipsterError> for AppError {
    fn from(err: clipster_common::error::ClipsterError) -> Self {
        match &err {
            clipster_common::error::ClipsterError::NotFound(_) => Self {
                status: StatusCode::NOT_FOUND,
                message: err.to_string(),
                internal: None,
            },
            clipster_common::error::ClipsterError::Duplicate => Self::duplicate(),
            clipster_common::error::ClipsterError::Unauthorized => Self::unauthorized(),
            clipster_common::error::ClipsterError::BadRequest(_) => Self {
                status: StatusCode::BAD_REQUEST,
                message: err.to_string(),
                internal: None,
            },
            _ => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "internal server error".into(),
                internal: Some(err.to_string()),
            },
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal server error".into(),
            internal: Some(err.to_string()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Some(ref detail) = self.internal {
            tracing::error!(status = %self.status, detail, "request error");
        }
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}
