//! HTTP layer for media. Issue an upload ticket, finalize after upload, read back.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

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

#[derive(Deserialize)]
struct UnlockBody {
    viewer_id: i64,
}

#[derive(Deserialize)]
struct ViewerQuery {
    viewer_id: i64,
}

async fn create_upload(
    State(state): State<AppState>,
    Json(body): Json<NewUpload>,
) -> Result<(StatusCode, Json<UploadTicket>), ApiError> {
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

async fn unlock(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UnlockBody>,
) -> Result<Json<UnlockSummary>, ApiError> {
    Ok(Json(state.media.unlock(id, body.viewer_id).await?))
}

/// Returns an HLS manifest (`application/vnd.apple.mpegurl`) with presigned
/// segment URLs, or 402 if the viewer hasn't unlocked the asset.
async fn manifest(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<ViewerQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let body = state.media.manifest(id, q.viewer_id).await?;
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
