//! Auth HTTP routes: register, login, and a `me` probe demonstrating the
//! `AuthUser` extractor.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::auth::model::{
    AuthResponse, CurrentUser, EmailCheckRequest, EmailCheckResult, LoginRequest, RegisterRequest,
};
use crate::auth::Caller;
use crate::error::ApiError;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/check-email", post(check_email))
        .route("/auth/me", get(me))
}

/// Email-first login step: does an account exist for this email? Drives whether the
/// UI then asks for a password to sign in or to create an account.
async fn check_email(
    State(state): State<AppState>,
    Json(body): Json<EmailCheckRequest>,
) -> Result<Json<EmailCheckResult>, ApiError> {
    let exists = state.auth.email_exists(&body.email).await?;
    Ok(Json(EmailCheckResult { exists }))
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
