//! Auth HTTP routes: register, login, and a `me` probe demonstrating the
//! `AuthUser` extractor.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::model::{AuthResponse, CurrentUser, LoginRequest, RegisterRequest};
use crate::auth::Caller;
use crate::error::ApiError;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/me", get(me))
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    let resp = state.auth.register(body).await?;
    Ok((StatusCode::CREATED, Json(resp)))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    Ok(Json(state.auth.login(body).await?))
}

/// Requires a valid bearer token; returns the caller's id and role so the frontend
/// can gate operator-only UI (via the `Caller` extractor).
async fn me(Caller(caller): Caller) -> Json<CurrentUser> {
    Json(CurrentUser {
        user_id: caller.user_id,
        role: caller.role,
    })
}
