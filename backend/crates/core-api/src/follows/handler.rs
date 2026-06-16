//! HTTP layer for follows.
//!
//! Following/unfollowing acts as the authenticated user (`/me/following/...`),
//! so a client can't manage someone else's graph. The follow list of any user is
//! public to read.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::follows::model::Follow;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/following/:followee_id", put(follow).delete(unfollow))
        .route("/users/:id/following", get(list_following))
}

async fn follow(
    AuthUser(follower): AuthUser,
    State(state): State<AppState>,
    Path(followee): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state.follows.follow(follower, followee).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unfollow(
    AuthUser(follower): AuthUser,
    State(state): State<AppState>,
    Path(followee): Path<i64>,
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
