//! HTTP layer for users: translate requests into service calls and back.
//! No business logic, no SQL — those live in `service` and `repository`.
//!
//! There is intentionally NO public `POST /users`: account creation is
//! `/auth/register` (which always creates an unverified user). The bot-gate is
//! mutated only here, via an operator-only endpoint.

use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};

use crate::auth::AdminUser;
use crate::error::ApiError;
use crate::state::AppState;
use crate::users::model::{ReferralTerms, ReferralTermsRequest, User, VerificationRequest};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users/:id", get(get_user))
        .route("/users/:id/verification", put(set_verification))
        .route("/users/:id/referral-terms", put(set_referral_terms))
}

async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<User>, ApiError> {
    let user = state.users.get(id).await?;
    Ok(Json(user))
}

/// Operator-only: set a user's bot-gate (verified) flag. `AdminUser` rejects the
/// unauthenticated (401) and non-operators (403) before this runs. This is the
/// ONLY way the gate can be flipped — it is never self-asserted at registration.
async fn set_verification(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<VerificationRequest>,
) -> Result<Json<User>, ApiError> {
    let user = state.users.set_verification(id, body.verified).await?;
    Ok(Json(user))
}

/// Operator-only: set a creator's referral contract (e.g. 5% for a negotiated
/// period). Frozen terms on existing referrals stay untouched — the contract
/// applies to future recruits (P-2).
async fn set_referral_terms(
    AdminUser(operator_id): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReferralTermsRequest>,
) -> Result<Json<ReferralTerms>, ApiError> {
    let terms = state
        .users
        .set_referral_terms(
            operator_id,
            id,
            body.bps,
            body.duration_epochs,
            body.note.as_deref(),
        )
        .await?;
    Ok(Json(terms))
}
