//! HTTP layer for interaction capture. `POST /interactions` appends one event.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use crate::error::ApiError;
use crate::interactions::model::{InteractionView, NewInteraction};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/interactions", post(record))
}

async fn record(
    State(state): State<AppState>,
    Json(body): Json<NewInteraction>,
) -> Result<(StatusCode, Json<InteractionView>), ApiError> {
    let event = state.interactions.record(body).await?;
    Ok((
        StatusCode::CREATED,
        Json(InteractionView::from_event(&event)),
    ))
}
