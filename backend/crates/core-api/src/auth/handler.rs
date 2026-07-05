//! Auth HTTP routes: register, login, and a `me` probe demonstrating the
//! `AuthUser` extractor.

use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
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
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

/// Revoke the session on this request (logout). Requires a valid token (`Caller`
/// → 401 otherwise); the token to revoke is read from the same Authorization
/// header. Returns 204 whether or not the row still existed (idempotent).
async fn logout(
    State(state): State<AppState>,
    _caller: Caller,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if let Some(token) = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        state.auth.logout(token).await?;
    }
    Ok(StatusCode::NO_CONTENT)
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

/// Requires a valid bearer token; returns the caller's id, role (gates
/// operator-only UI) and own referral code (for the share link, P-2).
async fn me(
    State(state): State<AppState>,
    Caller(caller): Caller,
) -> Result<Json<CurrentUser>, ApiError> {
    let referral_code = state.auth.referral_code_of(caller.user_id).await?;
    Ok(Json(CurrentUser {
        user_id: caller.user_id,
        role: caller.role,
        referral_code,
    }))
}
