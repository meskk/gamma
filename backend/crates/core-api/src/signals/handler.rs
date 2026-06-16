//! HTTP layer for content signals: the ingestion service's write-back endpoint.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::put;
use axum::{Json, Router};

use crate::auth::AdminUser;
use crate::error::ApiError;
use crate::signals::model::SignalWriteback;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/posts/:id/signals", put(write_signals))
}

/// Operator-only: the ingestion service writes back its analysis of a post. This
/// keeps DB writes behind the API — the service never touches Postgres directly.
/// `AdminUser` rejects the unauthenticated (401) and non-operators (403).
async fn write_signals(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(post_id): Path<i64>,
    Json(body): Json<SignalWriteback>,
) -> Result<StatusCode, ApiError> {
    state
        .signals
        .record(post_id, body.model_version, body.signals)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
