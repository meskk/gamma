//! HTTP layer for media. Issue an upload ticket, finalize after upload, read back.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::media::model::{MediaAssetView, NewUpload, UploadTicket};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/media", post(create_upload))
        .route("/media/:id/finalize", post(finalize))
        .route("/media/:id/transcode", post(transcode))
        .route("/media/:id", get(get_media))
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

async fn get_media(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<MediaAssetView>, ApiError> {
    Ok(Json(state.media.get(id).await?))
}
