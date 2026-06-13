//! Auth request/response shapes.

use serde::{Deserialize, Serialize};

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
