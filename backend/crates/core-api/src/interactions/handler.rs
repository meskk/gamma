//! HTTP layer for interaction capture. `POST /interactions` appends one event;
//! `DELETE /interactions` retracts a like (the un-like, ADR 0012) — same body
//! shape, so the client sends the exact tuple it liked.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::interactions::model::{InteractionView, NewInteraction};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/interactions", post(record).delete(retract))
}

async fn record(
    AuthUser(actor_id): AuthUser,
    State(state): State<AppState>,
    Json(mut body): Json<NewInteraction>,
) -> Result<(StatusCode, Json<InteractionView>), ApiError> {
    body.actor_id = actor_id; // authenticated identity, not client-supplied
    let event = state.interactions.record(body).await?;
    Ok((
        StatusCode::CREATED,
        Json(InteractionView::from_event(&event)),
    ))
}

/// Idempotent: un-liking something that was never (or is no longer) liked is a
/// 204 too — a toggle double-fire must not error, and the response being
/// independent of the target's existence opens no oracle.
async fn retract(
    AuthUser(actor_id): AuthUser,
    State(state): State<AppState>,
    Json(mut body): Json<NewInteraction>,
) -> Result<StatusCode, ApiError> {
    body.actor_id = actor_id; // authenticated identity, not client-supplied
    state.interactions.retract(body).await?;
    Ok(StatusCode::NO_CONTENT)
}
