//! HTTP layer for content signals: the ingestion service's write-back endpoint
//! and an operator-only read-back for inspecting what was stored.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::put;
use axum::{Json, Router};

use crate::auth::AdminUser;
use crate::error::ApiError;
use crate::signals::model::{ContentSignal, SignalWriteback};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/posts/:id/signals", put(write_signals).get(read_signals))
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

/// Operator-only: read back the stored signals for a post (404 if none yet). Pure
/// observability — for the pipeline/operators to inspect exactly what was written,
/// before any consumer exists. NOT a frontend contract: `ContentSignal` is
/// Serialize-only (no ts-rs export), so the signal shape stays unfrozen (ADR 0006).
async fn read_signals(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(post_id): Path<i64>,
) -> Result<Json<ContentSignal>, ApiError> {
    Ok(Json(state.signals.get(post_id).await?))
}
