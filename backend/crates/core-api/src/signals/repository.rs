//! Postgres persistence for content signals — the only place that knows the
//! `content_signals` / `post_embeddings` SQL. Upsert (write-back) and read
//! (for the M2.7 feed consumer).

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

    /// Store (or replace) the latest signals — and, if delivered, the post's
    /// embedding — in ONE transaction, so a write-back is all-or-nothing and a
    /// retry can never leave signals and embedding from different analyses.
    /// A second write-back for the same post supersedes the previous one
    /// (ADR 0009 §4: one current row per post; the single-worker invariant
    /// plus the version-targeted backfill replace a DB-side version guard).
    /// A non-existent post hits the FK constraint, surfaced as a client error
    /// by the service.
    pub async fn upsert(
        &self,
        post_id: i64,
        model_version: &str,
        schema_version: i16,
        signals: &Value,
        embedding: Option<&[f32]>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            r#"
            INSERT INTO content_signals (post_id, model_version, schema_version, signals, updated_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (post_id) DO UPDATE
            SET model_version = EXCLUDED.model_version,
                schema_version = EXCLUDED.schema_version,
                signals = EXCLUDED.signals,
                updated_at = now()
            "#,
            post_id,
            model_version,
            schema_version,
            signals
        )
        .execute(&mut *tx)
        .await?;

        if let Some(embedding) = embedding {
            sqlx::query!(
                r#"
                INSERT INTO post_embeddings (post_id, model_version, dim, embedding, updated_at)
                VALUES ($1, $2, $3, $4, now())
                ON CONFLICT (post_id) DO UPDATE
                SET model_version = EXCLUDED.model_version,
                    dim = EXCLUDED.dim,
                    embedding = EXCLUDED.embedding,
                    updated_at = now()
                "#,
                post_id,
                model_version,
                embedding.len() as i16,
                embedding
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    /// The latest signals for a post, if the pipeline has analysed it yet.
    /// Embeddings are deliberately NOT part of this read (ADR 0009 §3).
    pub async fn get(&self, post_id: i64) -> Result<Option<ContentSignal>, sqlx::Error> {
        sqlx::query_as!(
            ContentSignal,
            r#"
            SELECT post_id, model_version, schema_version, signals AS "signals: Value", updated_at
            FROM content_signals
            WHERE post_id = $1
            "#,
            post_id
        )
        .fetch_optional(&self.pool)
        .await
    }
}
