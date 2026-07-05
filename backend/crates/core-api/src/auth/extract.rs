//! Auth extractors. `AuthUser` yields the user id for any authenticated caller;
//! `AdminUser` additionally requires the operator role. Both reject missing or
//! invalid tokens with 401 before the handler runs; `AdminUser` rejects a valid
//! but non-operator caller with 403.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::auth::model::{Principal, Role};
use crate::error::ApiError;
use crate::state::AppState;

pub struct AuthUser(pub i64);

/// A caller proven to hold the `Operator` role. Use this on admin-only routes
/// (e.g. epoch settlement) instead of `AuthUser`.
pub struct AdminUser(pub i64);

/// A caller proven to be a machine SERVICE identity — or an operator (operators
/// can always do what services can, for manual repair). Use this on the seam
/// endpoints machines write through (signals write-back), NEVER on human-ops
/// routes: a leaked service credential must not be able to settle epochs, flip
/// bot gates, or take content down.
pub struct ServiceUser(pub i64);

/// The authenticated caller with their role. Use this (over `AuthUser`) when a
/// handler must decide access against the role too — e.g. a self-scoped read the
/// owner OR an operator may perform (`caller.0.can_act_as(id)`).
pub struct Caller(pub Principal);

/// Resolve the bearer token on the request to its principal, or 401 if the
/// header is missing/malformed or the token is unknown/expired.
async fn principal(parts: &mut Parts, state: &AppState) -> Result<Principal, ApiError> {
    let token = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;

    state
        .auth
        .authenticate(token)
        .await?
        .ok_or(ApiError::Unauthorized)
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(AuthUser(principal(parts, state).await?.user_id))
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Caller(principal(parts, state).await?))
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let caller = principal(parts, state).await?;
        if caller.role != Role::Operator {
            return Err(ApiError::Forbidden);
        }
        Ok(AdminUser(caller.user_id))
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for ServiceUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let caller = principal(parts, state).await?;
        if !matches!(caller.role, Role::Service | Role::Operator) {
            return Err(ApiError::Forbidden);
        }
        Ok(ServiceUser(caller.user_id))
    }
}
