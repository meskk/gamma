//! HTTP layer for comments: list a post's comments (public read) and add one
//! (authenticated). Translates requests to service calls; no logic, no SQL.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::comments::model::{Comment, NewComment};
use crate::error::ApiError;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/posts/:id/comments", get(list).post(create))
}

/// Any authenticated user can comment; the author is the session user.
async fn create(
    AuthUser(author_id): AuthUser,
    State(state): State<AppState>,
    Path(post_id): Path<i64>,
    Json(body): Json<NewComment>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    let comment = state.comments.create(post_id, author_id, body.body).await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

async fn list(
    State(state): State<AppState>,
    Path(post_id): Path<i64>,
) -> Result<Json<Vec<Comment>>, ApiError> {
    Ok(Json(state.comments.list(post_id).await?))
}
