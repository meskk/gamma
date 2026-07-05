//! Auth request/response shapes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A user's authorization role. Maps 1:1 to the Postgres `user_role` enum.
/// `User` is the default (non-privileged); `Operator` may run admin actions
/// such as epoch settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, TS)]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../bindings/")]
pub enum Role {
    User,
    Operator,
}

/// The authenticated caller behind a session token: who they are and what role
/// they hold. Resolved in one query so role checks cost no extra round-trip.
#[derive(Debug, Clone, Copy)]
pub struct Principal {
    pub user_id: i64,
    pub role: Role,
}

/// Response for `GET /auth/me`: the current session's user id and role (so the
/// frontend can gate operator-only navigation) plus the user's own referral
/// code (so the UI can render a share link, MASTERPLAN P-2).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct CurrentUser {
    pub user_id: i64,
    pub role: Role,
    pub referral_code: String,
}

impl Principal {
    /// May this caller read/act on a self-scoped resource owned by `target`?
    /// True for the resource's owner or any operator. Used to gate self-scoped
    /// reads (e.g. a user's own gem balance or feed).
    pub fn can_act_as(&self, target: i64) -> bool {
        self.user_id == target || self.role == Role::Operator
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub declared_categories: Vec<String>,
    /// Referral code of the user who invited this one (MASTERPLAN P-2). An
    /// UNKNOWN code fails the registration (400 invalid_referral_code) rather
    /// than being dropped silently — a typo must surface, not cost the
    /// referrer their cut.
    #[serde(default)]
    #[ts(optional)]
    pub referral_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Returned on register/login. The token is shown ONCE; only its hash is stored.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
}

/// Request for the email-first login step: enter the email, then the UI asks for a
/// password to sign in (if it exists) or to create an account (if it doesn't).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct EmailCheckRequest {
    pub email: String,
}

/// Whether an account already exists for the email. NOTE: this deliberately reveals
/// account existence (user enumeration) to enable the email-first UX — an accepted
/// product trade-off; the endpoint must stay rate-limited (see the per-route
/// rate-limit follow-up).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct EmailCheckResult {
    pub exists: bool,
}
