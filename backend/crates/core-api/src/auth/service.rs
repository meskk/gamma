//! Auth business logic: password hashing (argon2), token issuance, verification.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::auth::model::{AuthResponse, LoginRequest, Principal, RegisterRequest};
use crate::auth::repository::AuthRepository;
use crate::error::ApiError;
use db::PgPool;

/// How long a session is valid.
const SESSION_DAYS: i64 = 30;
/// Minimum password length.
const MIN_PASSWORD_LEN: usize = 8;

#[derive(Clone)]
pub struct AuthService {
    repo: AuthRepository,
}

impl AuthService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: AuthRepository::new(pool),
        }
    }

    pub async fn register(&self, req: RegisterRequest) -> Result<AuthResponse, ApiError> {
        let email = req.email.trim().to_lowercase();
        if !email.contains('@') || email.len() < 3 {
            return Err(ApiError::Validation("invalid_email"));
        }
        if req.password.len() < MIN_PASSWORD_LEN {
            return Err(ApiError::Validation("weak_password"));
        }

        let hash = hash_password(&req.password)?;
        let user_id = self
            .repo
            .create_user(&email, &hash, &req.declared_categories)
            .await
            .map_err(map_create_error)?;

        self.issue_session(user_id).await
    }

    pub async fn login(&self, req: LoginRequest) -> Result<AuthResponse, ApiError> {
        let email = req.email.trim().to_lowercase();
        let (user_id, hash) = self
            .repo
            .credentials_by_email(&email)
            .await?
            .ok_or(ApiError::Unauthorized)?;

        if !verify_password(&req.password, &hash) {
            return Err(ApiError::Unauthorized);
        }
        self.issue_session(user_id).await
    }

    /// Whether an account exists for `email` (the email-first login step). Normalises
    /// the email exactly as register/login do, so the check matches what a later
    /// login would look up.
    pub async fn email_exists(&self, email: &str) -> Result<bool, ApiError> {
        let email = email.trim().to_lowercase();
        Ok(self.repo.email_exists(&email).await?)
    }

    /// Resolve a bearer token to the authenticated principal (id + role), or
    /// `None` if the token is invalid/expired.
    pub async fn authenticate(&self, token: &str) -> Result<Option<Principal>, ApiError> {
        Ok(self
            .repo
            .principal_for_session(&hash_token(token))
            .await?
            .map(|(user_id, role)| Principal { user_id, role }))
    }

    async fn issue_session(&self, user_id: i64) -> Result<AuthResponse, ApiError> {
        let token = new_token();
        let expires_at = Utc::now() + Duration::days(SESSION_DAYS);
        self.repo
            .create_session(&hash_token(&token), user_id, expires_at)
            .await?;
        Ok(AuthResponse { token, user_id })
    }
}

/// A unique-violation on email means the address is taken.
fn map_create_error(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::UniqueViolation) {
            return ApiError::Conflict("email_taken");
        }
    }
    ApiError::Database(err)
}

fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(e.to_string()))
}

fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A fresh 256-bit random session token, hex-encoded.
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// SHA-256 of the token — only this is stored, so a DB leak can't be replayed.
fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
