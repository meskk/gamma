//! HTTP layer for the feed. `GET /users/:id/feed?limit=N` returns the viewer's
//! ranked posts.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::Caller;
use crate::error::ApiError;
use crate::feed::model::FeedPage;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/users/:id/feed", get(feed))
}

#[derive(Deserialize)]
struct FeedParams {
    #[serde(default = "default_limit")]
    limit: usize,
    /// Opaque continuation cursor from the previous page's `next_cursor` (B1).
    cursor: Option<String>,
}

fn default_limit() -> usize {
    50
}

/// A personalized feed is self-scoped: only its owner (or an operator) may read it.
async fn feed(
    Caller(caller): Caller,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Query(params): Query<FeedParams>,
) -> Result<Json<FeedPage>, ApiError> {
    if !caller.can_act_as(user_id) {
        return Err(ApiError::Forbidden);
    }
    let page = state
        .feed
        .personalized(user_id, params.limit, params.cursor.as_deref())
        .await?;
    Ok(Json(page))
}
