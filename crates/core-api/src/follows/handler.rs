//! HTTP layer for follows, nested under the user resource.
//!
//! The follower segment is named `:id` to match the users routes' `:id` — axum's
//! router requires a consistent parameter name at the same path position.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::follows::model::Follow;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/users/:id/following/:followee_id",
            put(follow).delete(unfollow),
        )
        .route("/users/:id/following", get(list_following))
}

async fn follow(
    State(state): State<AppState>,
    Path((follower, followee)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    state.follows.follow(follower, followee).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unfollow(
    State(state): State<AppState>,
    Path((follower, followee)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    state.follows.unfollow(follower, followee).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_following(
    State(state): State<AppState>,
    Path(follower): Path<i64>,
) -> Result<Json<Vec<Follow>>, ApiError> {
    Ok(Json(state.follows.list_following(follower).await?))
}
