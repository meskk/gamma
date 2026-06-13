//! `AuthUser` extractor — verifies the bearer token and yields the user id.
//! Any handler that takes `AuthUser` is authenticated; missing/invalid tokens
//! are rejected with 401 before the handler runs.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::error::ApiError;
use crate::state::AppState;

pub struct AuthUser(pub i64);

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ApiError::Unauthorized)?;

        let user_id = state
            .auth
            .authenticate(token)
            .await?
            .ok_or(ApiError::Unauthorized)?;
        Ok(AuthUser(user_id))
    }
}
