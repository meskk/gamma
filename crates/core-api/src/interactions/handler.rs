//! HTTP layer for interaction capture. `POST /interactions` appends one event.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::interactions::model::{InteractionView, NewInteraction};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/interactions", post(record))
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
