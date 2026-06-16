//! HTTP layer for posts. Translates requests to service calls; no logic, no SQL.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::posts::model::{NewPost, Post};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/posts", post(create_post).get(list_posts))
        .route("/posts/:id", get(get_post))
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
