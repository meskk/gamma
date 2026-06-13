//! HTTP layer for the feed. `GET /users/:id/feed?limit=N` returns the viewer's
//! ranked posts.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::error::ApiError;
use crate::posts::model::Post;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/users/:id/feed", get(feed))
}

#[derive(Deserialize)]
struct FeedParams {
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

async fn feed(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Query(params): Query<FeedParams>,
) -> Result<Json<Vec<Post>>, ApiError> {
    let posts = state.feed.personalized(user_id, params.limit).await?;
    Ok(Json(posts))
}
