//! HTTP layer for posts. Translates requests to service calls; no logic, no SQL.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::{AdminUser, AuthUser};
use crate::error::ApiError;
use crate::posts::model::{NewPost, Post, ReportRequest, ReportedPost};
use crate::posts::service::BackfillResult;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/posts", post(create_post).get(list_posts))
        .route("/posts/:id", get(get_post))
        // Moderation: any user reports; operators take down / restore / review.
        .route("/posts/:id/report", post(report))
        .route("/posts/:id/takedown", post(takedown))
        .route("/posts/:id/restore", post(restore))
        .route("/reports", get(list_reports))
        // Operator-only: sweep the existing corpus into the ingestion pipeline.
        .route("/admin/ingestion/backfill", post(backfill_ingestion))
}

#[derive(Deserialize)]
struct BackfillParams {
    /// Resume cursor: only posts with id greater than this are considered.
    #[serde(default)]
    after: i64,
    #[serde(default = "default_backfill_limit")]
    limit: i64,
}

fn default_backfill_limit() -> i64 {
    500
}

#[derive(Deserialize)]
struct ListParams {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn create_post(
    AuthUser(author_id): AuthUser,
    State(state): State<AppState>,
    Json(mut body): Json<NewPost>,
) -> Result<(StatusCode, Json<Post>), ApiError> {
    body.author_id = author_id; // authenticated identity, not client-supplied
    let post = state.posts.create(body).await?;
    Ok((StatusCode::CREATED, Json(post)))
}

async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, ApiError> {
    let post = state.posts.get(id).await?;
    Ok(Json(post))
}

async fn list_posts(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<Post>>, ApiError> {
    let posts = state.posts.list_recent(params.limit).await?;
    Ok(Json(posts))
}

/// Any authenticated user can report a post (idempotent per reporter).
async fn report(
    AuthUser(reporter_id): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReportRequest>,
) -> Result<StatusCode, ApiError> {
    state.posts.report(id, reporter_id, body.reason).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Operator-only: take a post down (it drops out of the feed and public reads).
async fn takedown(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, ApiError> {
    Ok(Json(state.posts.set_visibility(id, true).await?))
}

/// Operator-only: restore a previously taken-down post.
async fn restore(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, ApiError> {
    Ok(Json(state.posts.set_visibility(id, false).await?))
}

/// Operator-only review queue: reported posts with their report counts.
async fn list_reports(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ReportedPost>>, ApiError> {
    Ok(Json(state.posts.list_reported(100).await?))
}

/// Operator-only: enqueue not-yet-analysed posts so the ingestion pipeline sweeps
/// the existing corpus. Paginate by calling with `?after=<last_id>` until the
/// response reports `enqueued == 0`. Enqueue-only — it never touches the signal
/// shape or the feed (ADR 0006).
async fn backfill_ingestion(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(params): Query<BackfillParams>,
) -> Result<Json<BackfillResult>, ApiError> {
    Ok(Json(
        state
            .posts
            .backfill_unanalyzed(params.after, params.limit)
            .await?,
    ))
}
