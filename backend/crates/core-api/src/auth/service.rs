//! Auth business logic: password hashing (argon2), token issuance, verification.

use std::sync::LazyLock;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::auth::model::{AuthResponse, LoginRequest, Principal, RegisterRequest};
use crate::auth::repository::AuthRepository;
use crate::auth::throttle;
use crate::error::ApiError;
use db::PgPool;

/// How long a session is valid.
const SESSION_DAYS: i64 = 30;
/// Minimum password length.
const MIN_PASSWORD_LEN: usize = 8;

/// A valid argon2 hash of a throwaway password, computed once. `login` verifies
/// against it when the email is unknown, so a failed login runs the SAME argon2
/// work whether or not the account exists — closing the response-latency oracle
/// that would otherwise reveal which emails are registered.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| hash_password("timing-equalizer-not-a-real-password").expect("dummy hash"));

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

        // argon2 is deliberately CPU/memory-heavy; run it on the blocking pool so a
        // burst of registrations can't starve the async runtime's worker threads
        // (which would stall every other request, including /health).
        let password = req.password;
        let hash = spawn_hash(move || hash_password(&password)).await?;
        let user_id = self
            .repo
            .create_user(&email, &hash, &req.declared_categories)
            .await
            .map_err(map_create_error)?;

        self.issue_session(user_id).await
    }

    pub async fn login(&self, req: LoginRequest) -> Result<AuthResponse, ApiError> {
        let email = req.email.trim().to_lowercase();

        // Throttle fast-path BEFORE the argon2 work. Unknown emails accumulate
        // throttle rows exactly like real ones (keyed by email, see migration
        // 0017), so this adds no enumeration or timing oracle — and a locked
        // email can't burn our CPU. A locked attempt does NOT count as a new
        // failure, so retrying can't extend a lock (griefing stays bounded).
        let throttle_row = self.repo.throttle_state(&email).await?;
        if let Some((_, Some(locked_until))) = throttle_row {
            let now = Utc::now();
            if locked_until > now {
                return Err(ApiError::TooManyRequests {
                    retry_after_secs: retry_after_secs(locked_until, now),
                });
            }
        }

        let creds = self.repo.credentials_by_email(&email).await?;

        // Always run exactly one argon2 verify — against a dummy hash when the email
        // is unknown — so response latency is the same for existing and non-existing
        // accounts (no enumeration oracle). And run it off the async runtime
        // (spawn_blocking) so concurrent logins can't exhaust the worker threads.
        let (user_id, hash) = match creds {
            Some((id, h)) => (Some(id), h),
            None => (None, DUMMY_HASH.clone()),
        };
        let password = req.password;
        let ok = spawn_hash(move || Ok(verify_password(&password, &hash))).await?;

        match user_id {
            Some(id) if ok => {
                // Success forgets the failure history (only if there was any —
                // the common no-failure login stays a pure read path).
                if throttle_row.is_some() {
                    self.repo.clear_login_throttle(&email).await?;
                }
                self.issue_session(id).await
            }
            _ => {
                // Count the failure; from the policy threshold on, set the lock.
                let count = self.repo.record_login_failure(&email).await?;
                if let Some(lock) = throttle::lock_duration(count.try_into().unwrap_or(0)) {
                    let until = Utc::now() + Duration::seconds(lock.as_secs() as i64);
                    self.repo.set_login_lock(&email, until).await?;
                }
                Err(ApiError::Unauthorized)
            }
        }
    }

    /// Revoke the session behind this bearer token (logout). Idempotent: revoking an
    /// already-gone token is a no-op.
    pub async fn logout(&self, token: &str) -> Result<(), ApiError> {
        self.repo.delete_session(&hash_token(token)).await?;
        Ok(())
    }

    /// Purge expired sessions (housekeeping). Returns how many were removed.
    pub async fn purge_expired_sessions(&self) -> Result<u64, ApiError> {
        Ok(self.repo.delete_expired_sessions().await?)
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

/// Whole seconds until `locked_until`, rounded UP so a client that waits the
/// advertised `Retry-After` is never rejected again, and never below 1.
fn retry_after_secs(locked_until: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    let ms = (locked_until - now).num_milliseconds().max(0);
    (((ms + 999) / 1000).max(1)) as u64
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

/// Run a CPU-heavy argon2 closure on tokio's blocking pool, flattening the join
/// error into an `ApiError` so callers just `?` it.
async fn spawn_hash<T, F>(f: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::Internal(format!("password hashing task failed: {e}")))?
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
