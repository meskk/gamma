//! HTTP layer for settlement and gem balances.
//! `POST /epochs/:epoch_k/settle` runs the epoch worker; `GET /users/:id/gems`
//! reads a balance.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::error::ApiError;
use crate::gems::model::{GemBalance, SettlementSummary};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/epochs/:epoch_k/settle", post(settle))
        .route("/users/:id/gems", get(gem_balance))
}

async fn settle(
    State(state): State<AppState>,
    Path(epoch_k): Path<i64>,
) -> Result<Json<SettlementSummary>, ApiError> {
    Ok(Json(state.gems.settle(epoch_k).await?))
}

async fn gem_balance(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<GemBalance>, ApiError> {
    Ok(Json(state.gems.gem_balance(user_id).await?))
}
