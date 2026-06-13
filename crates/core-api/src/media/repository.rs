//! Postgres-backed media repository — the only place that knows media SQL.

use crate::media::model::MediaAsset;
use db::PgPool;

#[derive(Clone)]
pub struct MediaRepository {
    pool: PgPool,
}

impl MediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        owner_id: i64,
        kind: &str,
        object_key: &str,
        content_type: &str,
    ) -> Result<MediaAsset, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            INSERT INTO media_assets (owner_id, kind, object_key, content_type, status)
            VALUES ($1, $2, $3, $4, 'pending')
            RETURNING id, owner_id, kind, object_key, content_type, status, size_bytes, created_at
            "#,
            owner_id,
            kind,
            object_key,
            content_type
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn get(&self, id: i64) -> Result<Option<MediaAsset>, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            SELECT id, owner_id, kind, object_key, content_type, status, size_bytes, created_at
            FROM media_assets
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Mark an asset ready and record its uploaded size.
    pub async fn mark_ready(&self, id: i64, size_bytes: i64) -> Result<MediaAsset, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            UPDATE media_assets
            SET status = 'ready', size_bytes = $2
            WHERE id = $1
            RETURNING id, owner_id, kind, object_key, content_type, status, size_bytes, created_at
            "#,
            id,
            size_bytes
        )
        .fetch_one(&self.pool)
        .await
    }
}
