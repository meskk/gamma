//! Auth request/response shapes.

use serde::{Deserialize, Serialize};

/// A user's authorization role. Maps 1:1 to the Postgres `user_role` enum.
/// `User` is the default (non-privileged); `Operator` may run admin actions
/// such as epoch settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
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

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub declared_categories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Returned on register/login. The token is shown ONCE; only its hash is stored.
#[derive(Debug, Clone, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user_id: i64,
}
