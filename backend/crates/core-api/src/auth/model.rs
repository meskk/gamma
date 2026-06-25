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

/// Response for `GET /auth/me`: the current session's user id and role, so the
/// frontend can gate operator-only navigation and routes.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct CurrentUser {
    pub user_id: i64,
    pub role: Role,
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
