//! HTTP layer for media. Issue an upload ticket, finalize after upload, read back.
//!
//! Upload, unlock and manifest derive the acting user from the authenticated
//! session (`AuthUser`); finalize/transcode/get are keyed by asset id.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::media::model::{MediaAssetView, NewUpload, UnlockSummary, UploadTicket};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/media", post(create_upload))
        .route("/media/:id/finalize", post(finalize))
        .route("/media/:id/transcode", post(transcode))
        .route("/media/:id/unlock", post(unlock))
        .route("/media/:id/manifest", get(manifest))
        .route("/media/:id", get(get_media))
}

async fn create_upload(
    AuthUser(owner_id): AuthUser,
    State(state): State<AppState>,
    Json(mut body): Json<NewUpload>,
) -> Result<(StatusCode, Json<UploadTicket>), ApiError> {
    body.owner_id = owner_id; // authenticated identity, not client-supplied
    let ticket = state.media.create_upload(body).await?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

async fn finalize(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<MediaAssetView>, ApiError> {
    Ok(Json(state.media.finalize(id).await?))
}

async fn transcode(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<MediaAssetView>, ApiError> {
    Ok(Json(state.media.transcode(id).await?))
}

/// Spend the authenticated viewer's gems to unlock the asset.
async fn unlock(
    AuthUser(viewer_id): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<UnlockSummary>, ApiError> {
    Ok(Json(state.media.unlock(id, viewer_id).await?))
}

/// Returns an HLS manifest (`application/vnd.apple.mpegurl`) with presigned
/// segment URLs, or 402 if the authenticated viewer hasn't unlocked the asset.
async fn manifest(
    AuthUser(viewer_id): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let body = state.media.manifest(id, viewer_id).await?;
    Ok((
        [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
        body,
    ))
}

async fn get_media(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<MediaAssetView>, ApiError> {
    Ok(Json(state.media.get(id).await?))
}
