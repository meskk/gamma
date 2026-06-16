//! HTTP layer for settlement and gem balances.
//! `POST /epochs/:epoch_k/settle` runs the epoch worker; `GET /users/:id/gems`
//! reads a balance.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::AdminUser;
use crate::error::ApiError;
use crate::gems::model::{GemBalance, SettlementSummary};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/epochs/:epoch_k/settle", post(settle))
        .route("/users/:id/gems", get(gem_balance))
}

/// Operator-only: minting an epoch's gems must not be caller-triggerable by
/// anyone. The `AdminUser` extractor rejects non-operators (403) and the
/// unauthenticated (401) before this runs.
async fn settle(
    _admin: AdminUser,
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
