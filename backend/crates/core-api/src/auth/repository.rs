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
