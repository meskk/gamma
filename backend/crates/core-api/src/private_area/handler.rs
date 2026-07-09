//! HTTP layer for the private-area configuration (P-4/A3, ADR 0011 §6).
//!
//! These routes exist ONLY when the `GAMMA_PRIVATE_AREA` flag is on (default
//! OFF; live only after legal sign-off). While it is off `lib.rs` never mounts
//! them, so the paths are genuinely nonexistent — a probe cannot tell the dark
//! feature apart from an unrouted URL (no `Allow` header, no JSON body, no
//! 401/400 tell). The creator id for writes comes from the authenticated
//! session, never from the path or body.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::private_area::model::{PrivateAreaConfigRequest, PrivateAreaView};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me/private-area", get(get_own).put(configure))
        .route("/users/:id/private-area", get(get_terms))
}

async fn configure(
    AuthUser(creator_id): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<PrivateAreaConfigRequest>,
) -> Result<Json<PrivateAreaView>, ApiError> {
    Ok(Json(state.private_area.configure(creator_id, body).await?))
}

async fn get_own(
    AuthUser(creator_id): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<PrivateAreaView>, ApiError> {
    Ok(Json(state.private_area.get(creator_id).await?))
}

/// Any authenticated viewer may read a creator's terms — they ARE the offer
/// the fourth tab renders (A7).
async fn get_terms(
    AuthUser(_viewer_id): AuthUser,
    State(state): State<AppState>,
    Path(creator_id): Path<i64>,
) -> Result<Json<PrivateAreaView>, ApiError> {
    Ok(Json(state.private_area.get(creator_id).await?))
}
