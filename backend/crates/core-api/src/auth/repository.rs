//! Auth persistence: user credentials and sessions.

use chrono::{DateTime, Utc};
use db::PgPool;

use crate::auth::model::Role;

#[derive(Clone)]
pub struct AuthRepository {
    pool: PgPool,
}

impl AuthRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a user with credentials. Returns the new id, or a unique-violation
    /// error if the email is taken (mapped to 409 by the service).
    pub async fn create_user(
        &self,
        email: &str,
        password_hash: &str,
        declared_categories: &[String],
    ) -> Result<i64, sqlx::Error> {
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, password_hash, declared_categories, bot_gate_v)
            VALUES ($1, $2, $3, false)
            RETURNING id
            "#,
            email,
            password_hash,
            declared_categories
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    /// (id, password_hash) for a login email, if one exists with a password set.
    pub async fn credentials_by_email(
        &self,
        email: &str,
    ) -> Result<Option<(i64, String)>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT id, password_hash FROM users WHERE email = $1"#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|r| r.password_hash.map(|h| (r.id, h))))
    }

    /// Whether an account exists for this (already-normalised) email. Powers the
    /// email-first login step.
    pub async fn email_exists(&self, email: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM users WHERE email = $1) AS "exists!""#,
            email
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn create_session(
        &self,
        token_hash: &str,
        user_id: i64,
        expires_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)"#,
            token_hash,
            user_id,
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a session by its token hash (logout / revocation). Idempotent — a
    /// missing row affects zero rows and is not an error.
    pub async fn delete_session(&self, token_hash: &str) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM sessions WHERE token_hash = $1", token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete all expired sessions (housekeeping; the `sessions_expires_at` index in
    /// migration 0016 keeps this cheap). Returns how many were removed.
    pub async fn delete_expired_sessions(&self) -> Result<u64, sqlx::Error> {
        let res = sqlx::query!("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Resolve a referral code to its owner's user id, if the code exists.
    pub async fn user_id_by_referral_code(&self, code: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar!("SELECT id FROM users WHERE referral_code = $1", code)
            .fetch_optional(&self.pool)
            .await
    }

    /// A user's own referral code (every user has one, DB-generated).
    pub async fn referral_code_of(&self, user_id: i64) -> Result<String, sqlx::Error> {
        sqlx::query_scalar!("SELECT referral_code FROM users WHERE id = $1", user_id)
            .fetch_one(&self.pool)
            .await
    }

    /// The operator-set contract override for a referrer, if any:
    /// `(bps, duration_epochs)`. Absent ⇒ the econ-params defaults apply.
    pub async fn referral_terms_for(
        &self,
        referrer_id: i64,
    ) -> Result<Option<(i32, i64)>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT bps, duration_epochs FROM referral_terms WHERE referrer_id = $1"#,
            referrer_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.bps, r.duration_epochs)))
    }

    /// Record who referred a new user, with the terms FROZEN at registration
    /// (bps + last earning epoch). One referrer per referred user (PK).
    pub async fn create_referral(
        &self,
        referred_id: i64,
        referrer_id: i64,
        bps: i32,
        valid_until_epoch: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO referrals (referred_id, referrer_id, bps, valid_until_epoch)
            VALUES ($1, $2, $3, $4)
            "#,
            referred_id,
            referrer_id,
            bps,
            valid_until_epoch
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Current throttle state for a (normalised) login email:
    /// `(failed_count, locked_until)`, or `None` if the email has no row.
    pub async fn throttle_state(
        &self,
        email: &str,
    ) -> Result<Option<(i32, Option<DateTime<Utc>>)>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT failed_count, locked_until FROM login_throttle WHERE email = $1"#,
            email
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.failed_count, r.locked_until)))
    }

    /// Count one more failed login for this email and return the NEW count.
    /// Atomic upsert, so concurrent failures both count instead of losing one.
    pub async fn record_login_failure(&self, email: &str) -> Result<i32, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            INSERT INTO login_throttle (email, failed_count, last_failed_at)
            VALUES ($1, 1, now())
            ON CONFLICT (email) DO UPDATE
            SET failed_count = login_throttle.failed_count + 1, last_failed_at = now()
            RETURNING failed_count
            "#,
            email
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Set the lock on an email (computed by the throttle policy from the count
    /// `record_login_failure` returned).
    pub async fn set_login_lock(
        &self,
        email: &str,
        locked_until: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE login_throttle SET locked_until = $2 WHERE email = $1"#,
            email,
            locked_until
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forget the throttle state for an email (successful login). Idempotent.
    pub async fn clear_login_throttle(&self, email: &str) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM login_throttle WHERE email = $1", email)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Drop throttle rows with no failure for 24h (housekeeping; the
    /// `login_throttle_last_failed_at` index keeps this cheap). Returns how many.
    pub async fn sweep_stale_login_throttle(&self) -> Result<u64, sqlx::Error> {
        let res = sqlx::query!(
            "DELETE FROM login_throttle WHERE last_failed_at <= now() - interval '24 hours'"
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// A user id for an email (any account), or `None`. Used by the code flows,
    /// which — unlike `credentials_by_email` — don't need a password to be set.
    pub async fn user_id_by_email(&self, email: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar!(r#"SELECT id FROM users WHERE email = $1"#, email)
            .fetch_optional(&self.pool)
            .await
    }

    /// Store (or replace) the one-time code for (email, purpose). A conflicting
    /// row is overwritten ONLY if the previous code is older than `cooldown_secs`
    /// — so a rapid re-request within the cooldown is suppressed (bounds
    /// email-bombing). Returns whether a code was actually (re)written and should
    /// therefore be sent.
    pub async fn upsert_email_code(
        &self,
        email: &str,
        purpose: &str,
        code_hash: &str,
        expires_at: DateTime<Utc>,
        cooldown_secs: f64,
    ) -> Result<bool, sqlx::Error> {
        let written = sqlx::query_scalar!(
            r#"
            INSERT INTO email_codes (email, purpose, code_hash, expires_at, attempts, created_at)
            VALUES ($1, $2, $3, $4, 0, now())
            ON CONFLICT (email, purpose) DO UPDATE
            SET code_hash = EXCLUDED.code_hash, expires_at = EXCLUDED.expires_at,
                attempts = 0, created_at = now()
            WHERE email_codes.created_at <= now() - make_interval(secs => $5)
            RETURNING 1 AS "written!"
            "#,
            email,
            purpose,
            code_hash,
            expires_at,
            cooldown_secs
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(written.is_some())
    }

    /// Atomically spend ONE guess against the active code for (email, purpose)
    /// and return its `code_hash` — but only if a live, non-burned code exists
    /// (`attempts < max_attempts` AND not expired). Returns `None` otherwise
    /// (missing / expired / already burned). Doing the attempt-increment and the
    /// eligibility check in one UPDATE is what makes it race-safe: concurrent
    /// guesses can't all read `attempts = 0` and slip past the cap. Crucially it
    /// does NOT delete a burned code — the row survives so the request cooldown
    /// keyed on it still holds (deleting on burn would let a fresh, cooldown-free
    /// code be minted immediately).
    pub async fn claim_code_attempt(
        &self,
        email: &str,
        purpose: &str,
        max_attempts: i32,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            UPDATE email_codes
            SET attempts = attempts + 1
            WHERE email = $1 AND purpose = $2 AND attempts < $3 AND expires_at > now()
            RETURNING code_hash
            "#,
            email,
            purpose,
            max_attempts
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Consume (delete) the code for (email, purpose) on a correct guess. Returns
    /// whether a row was actually deleted, so exactly one of two concurrent
    /// correct submissions wins single-use.
    pub async fn consume_email_code(
        &self,
        email: &str,
        purpose: &str,
    ) -> Result<bool, sqlx::Error> {
        let deleted = sqlx::query_scalar!(
            r#"DELETE FROM email_codes WHERE email = $1 AND purpose = $2 RETURNING 1 AS "one!""#,
            email,
            purpose
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(deleted.is_some())
    }

    /// Delete expired codes (housekeeping; the `email_codes_expires_at` index
    /// keeps this cheap). Returns how many were removed.
    pub async fn delete_expired_email_codes(&self) -> Result<u64, sqlx::Error> {
        let res = sqlx::query!("DELETE FROM email_codes WHERE expires_at <= now()")
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Set a user's password hash (password reset).
    pub async fn set_password_hash(
        &self,
        user_id: i64,
        password_hash: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"UPDATE users SET password_hash = $2 WHERE id = $1"#,
            user_id,
            password_hash
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete every session for a user — a password reset logs out all devices
    /// (and any attacker who triggered the reset). Returns how many were removed.
    pub async fn delete_sessions_for_user(&self, user_id: i64) -> Result<u64, sqlx::Error> {
        let res = sqlx::query!("DELETE FROM sessions WHERE user_id = $1", user_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// The (user id, role) behind a live (unexpired) session token hash, or
    /// `None` if the token is unknown or expired. Joins `users` so a role check
    /// needs no second query.
    pub async fn principal_for_session(
        &self,
        token_hash: &str,
    ) -> Result<Option<(i64, Role)>, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT u.id, u.role AS "role: Role"
            FROM sessions s
            JOIN users u ON u.id = s.user_id
            WHERE s.token_hash = $1 AND s.expires_at > now()
            "#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.id, r.role)))
    }
}
