//! Auth business logic: password hashing (argon2), token issuance, verification.

use std::sync::{Arc, LazyLock};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::auth::code;
use crate::auth::model::{AuthResponse, CodePurpose, LoginRequest, Principal, RegisterRequest};
use crate::auth::repository::AuthRepository;
use crate::auth::throttle;
use crate::error::ApiError;
use crate::mailer::{LogMailer, Mailer};
use db::PgPool;

/// Minimum password length.
const MIN_PASSWORD_LEN: usize = 8;

/// How long a session is valid, in days (`GAMMA_SESSION_TTL_DAYS`, default 30).
fn session_ttl_days() -> i64 {
    crate::util::env_parsed("GAMMA_SESSION_TTL_DAYS", 30)
}

/// A valid argon2 hash of a throwaway password, computed once. `login` verifies
/// against it when the email is unknown, so a failed login runs the SAME argon2
/// work whether or not the account exists — closing the response-latency oracle
/// that would otherwise reveal which emails are registered.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| hash_password("timing-equalizer-not-a-real-password").expect("dummy hash"));

#[derive(Clone)]
pub struct AuthService {
    repo: AuthRepository,
    econ: econ_params::EconParams,
    /// The outbound-mail seam (recovery codes). Defaults to the dev `LogMailer`;
    /// the running app injects a real provider via `with_mailer` once one exists.
    mailer: Arc<dyn Mailer>,
}

impl AuthService {
    /// Default econ knobs — fine for tests; the running app threads the
    /// configured set through `with_econ` (ADR 0003).
    pub fn new(pool: PgPool) -> Self {
        Self::with_econ(pool, econ_params::EconParams::default())
    }

    pub fn with_econ(pool: PgPool, econ: econ_params::EconParams) -> Self {
        Self {
            repo: AuthRepository::new(pool),
            econ,
            mailer: Arc::new(LogMailer),
        }
    }

    /// Inject a mail provider (production wiring, and tests that capture codes).
    pub fn with_mailer(mut self, mailer: Arc<dyn Mailer>) -> Self {
        self.mailer = mailer;
        self
    }

    pub async fn register(&self, req: RegisterRequest) -> Result<AuthResponse, ApiError> {
        let email = req.email.trim().to_lowercase();
        if !email.contains('@') || email.len() < 3 {
            return Err(ApiError::Validation("invalid_email"));
        }
        if req.password.len() < MIN_PASSWORD_LEN {
            return Err(ApiError::Validation("weak_password"));
        }

        // Resolve the referral BEFORE creating the user: an unknown code fails
        // the whole registration (400) instead of silently costing the referrer
        // their cut on a typo.
        let referrer_id = match req
            .referral_code
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            Some(code) => Some(
                self.repo
                    .user_id_by_referral_code(code)
                    .await?
                    .ok_or(ApiError::Validation("invalid_referral_code"))?,
            ),
            None => None,
        };

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

        if let Some(referrer_id) = referrer_id {
            // Freeze the terms NOW: the referrer's operator-set contract if one
            // exists, else the econ-params defaults. valid_until is the last
            // epoch (inclusive) in which the referral earns. (Users can't be
            // deleted in 1a, so the FK between resolve and insert can't break.)
            let (bps, duration_epochs) = match self.repo.referral_terms_for(referrer_id).await? {
                Some(terms) => terms,
                None => (
                    i32::from(self.econ.referral_bps_default),
                    self.econ.referral_duration_epochs as i64,
                ),
            };
            let current = domain::Epoch::from_unix_seconds(Utc::now().timestamp());
            let valid_until = current.0 as i64 + duration_epochs;
            self.repo
                .create_referral(user_id, referrer_id, bps, valid_until)
                .await?;
        }

        self.issue_session(user_id).await
    }

    /// The caller's own referral code (for the share link in the UI).
    pub async fn referral_code_of(&self, user_id: i64) -> Result<String, ApiError> {
        Ok(self.repo.referral_code_of(user_id).await?)
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

    /// Drop login-throttle rows with no failure for 24h (housekeeping).
    /// Returns how many were removed.
    pub async fn sweep_stale_login_throttle(&self) -> Result<u64, ApiError> {
        Ok(self.repo.sweep_stale_login_throttle().await?)
    }

    /// Whether an account exists for `email` (the email-first login step). Normalises
    /// the email exactly as register/login do, so the check matches what a later
    /// login would look up.
    pub async fn email_exists(&self, email: &str) -> Result<bool, ApiError> {
        let email = email.trim().to_lowercase();
        Ok(self.repo.email_exists(&email).await?)
    }

    /// Request a one-time code by email (recovery entry point for BOTH
    /// passwordless login and password reset). Returns `Ok(())` whether or not
    /// the account exists — the handler answers 204 either way, so there is no
    /// enumeration oracle. A per-(email, purpose) cooldown (in the repo) plus the
    /// auth rate-limit layer bound abuse/bombing. Mail-send failures are logged,
    /// never surfaced (that too would leak existence).
    pub async fn request_code(&self, email: &str, purpose: CodePurpose) -> Result<(), ApiError> {
        let email = email.trim().to_lowercase();
        if self.repo.user_id_by_email(&email).await?.is_none() {
            return Ok(());
        }
        let plaintext = code::generate_code();
        let code_hash = sha256_hex(&plaintext);
        let expires_at = Utc::now() + Duration::seconds(code::CODE_TTL.as_secs() as i64);
        let should_send = self
            .repo
            .upsert_email_code(
                &email,
                purpose.as_str(),
                &code_hash,
                expires_at,
                code::REQUEST_COOLDOWN.as_secs() as f64,
            )
            .await?;
        if should_send {
            // Dispatch the mail OFF the request path: a real provider can block for
            // 100s of ms, and doing it inline would (a) stall a runtime worker and
            // (b) make response latency depend on send time — a loud account-
            // existence oracle. spawn_blocking decouples both. (A residual, sub-ms
            // timing delta remains between the exists branch — one extra INSERT —
            // and the early return for a non-existent account; accepted for 1a on
            // this rate-limited endpoint. We deliberately do NOT store a code row
            // for unknown emails: that would be an unbounded-growth / bombing sink.)
            let mailer = self.mailer.clone();
            let to = email.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(e) = mailer.send_code(&to, purpose, &plaintext) {
                    tracing::error!(error = %e, "failed to send email code");
                }
            });
        }
        Ok(())
    }

    /// Exchange an emailed code for a session (passwordless login).
    pub async fn login_with_code(&self, email: &str, code: &str) -> Result<AuthResponse, ApiError> {
        let email = email.trim().to_lowercase();
        let user_id = self.verify_code(&email, CodePurpose::Login, code).await?;
        self.issue_session(user_id).await
    }

    /// Set a new password using an emailed reset code, then return a fresh
    /// session. Every prior session for the account is invalidated (logs out
    /// other devices / any attacker), and the login throttle is cleared so the
    /// user isn't locked out right after resetting.
    pub async fn reset_password(
        &self,
        email: &str,
        code: &str,
        new_password: String,
    ) -> Result<AuthResponse, ApiError> {
        // Check password strength BEFORE touching the code, so a weak password
        // doesn't spend an attempt / consume a valid code.
        if new_password.len() < MIN_PASSWORD_LEN {
            return Err(ApiError::Validation("weak_password"));
        }
        let email = email.trim().to_lowercase();
        // Hash first (independent of the code) so the code is consumed only
        // immediately before the write that uses it — shrinking the window where
        // a consumed code is wasted by a later failure.
        let hash = spawn_hash(move || hash_password(&new_password)).await?;
        let user_id = self
            .verify_code(&email, CodePurpose::PasswordReset, code)
            .await?;
        self.repo.set_password_hash(user_id, &hash).await?;
        self.repo.delete_sessions_for_user(user_id).await?;
        self.repo.clear_login_throttle(&email).await?;
        self.issue_session(user_id).await
    }

    /// Verify a one-time code for (email, purpose) and, on success, CONSUME it
    /// (single-use) and return the user id. Every failure path returns the same
    /// generic `Unauthorized` — no oracle for unknown-email vs. missing/expired/
    /// burned/wrong-code.
    ///
    /// Race-safety: `claim_code_attempt` does the eligibility check AND the
    /// attempt increment in one atomic UPDATE, so concurrent guesses can't slip
    /// past `MAX_ATTEMPTS`, and a burned code is left in place (not deleted) so
    /// the request cooldown keyed on it keeps holding. The correct guess consumes
    /// the row via a delete-returning, so exactly one of two concurrent correct
    /// submissions wins.
    async fn verify_code(
        &self,
        email: &str,
        purpose: CodePurpose,
        code: &str,
    ) -> Result<i64, ApiError> {
        let purpose_str = purpose.as_str();
        let code_hash = match self
            .repo
            .claim_code_attempt(email, purpose_str, code::MAX_ATTEMPTS)
            .await?
        {
            Some(h) => h,
            None => return Err(ApiError::Unauthorized),
        };
        if sha256_hex(code) != code_hash {
            return Err(ApiError::Unauthorized);
        }
        // Correct code: consume it. Losing the delete race means another request
        // already redeemed it — reject, so single-use holds.
        if !self.repo.consume_email_code(email, purpose_str).await? {
            return Err(ApiError::Unauthorized);
        }
        self.repo
            .user_id_by_email(email)
            .await?
            .ok_or(ApiError::Unauthorized)
    }

    /// Drop expired email codes (housekeeping). Returns how many were removed.
    pub async fn sweep_expired_email_codes(&self) -> Result<u64, ApiError> {
        Ok(self.repo.delete_expired_email_codes().await?)
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
        let expires_at = Utc::now() + Duration::days(session_ttl_days());
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
    sha256_hex(token)
}

/// Hex-encoded SHA-256 of a string. Used to store session tokens and one-time
/// codes as hashes (never the plaintext), so a DB leak can't be replayed.
fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
