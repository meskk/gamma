//! Auth HTTP routes: register, login, and a `me` probe demonstrating the
//! `AuthUser` extractor.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::auth::model::{AuthResponse, LoginRequest, RegisterRequest};
use crate::auth::AuthUser;
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

#[derive(Serialize)]
struct Me {
    user_id: i64,
}

/// Requires a valid bearer token (via the `AuthUser` extractor).
async fn me(AuthUser(user_id): AuthUser) -> Json<Me> {
    Json(Me { user_id })
}
