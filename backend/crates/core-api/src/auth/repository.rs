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
