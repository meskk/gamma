//! Postgres persistence for content signals — the only place that knows the
//! `content_signals` SQL. Upsert (write-back) and read (for the future feed).

use serde_json::Value;

use crate::signals::model::ContentSignal;
use db::PgPool;

#[derive(Clone)]
pub struct ContentSignalRepository {
    pool: PgPool,
}

impl ContentSignalRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Store (or replace) the latest signals for a post. A second write-back for
    /// the same post supersedes the previous one. A non-existent post hits the FK
    /// constraint, surfaced as a client error by the service.
    pub async fn upsert(
        &self,
        post_id: i64,
        model_version: &str,
        signals: &Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO content_signals (post_id, model_version, signals, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (post_id) DO UPDATE
            SET model_version = EXCLUDED.model_version,
                signals = EXCLUDED.signals,
                updated_at = now()
            "#,
            post_id,
            model_version,
            signals
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The latest signals for a post, if the pipeline has analysed it yet.
    pub async fn get(&self, post_id: i64) -> Result<Option<ContentSignal>, sqlx::Error> {
        sqlx::query_as!(
            ContentSignal,
            r#"
            SELECT post_id, model_version, signals AS "signals: Value", updated_at
            FROM content_signals
            WHERE post_id = $1
            "#,
            post_id
        )
        .fetch_optional(&self.pool)
        .await
    }
}
