//! Postgres-backed post repository — the only place that knows posts SQL.
//! Same shape as the users repository (concrete struct, `query_as!` checked
//! queries). Adds `list_recent` to show the multi-row (`fetch_all`) template.

use chrono::{DateTime, Utc};

use crate::posts::model::{NewPost, Post, ReportedPost};
use db::PgPool;

#[derive(Clone)]
pub struct PostRepository {
    pool: PgPool,
}

impl PostRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, new: &NewPost) -> Result<Post, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            INSERT INTO posts (author_id, category, body)
            VALUES ($1, $2, $3)
            RETURNING id, author_id, category, body, created_at, popularity_score
            "#,
            new.author_id,
            new.category.as_deref(),
            new.body
        )
        .fetch_one(&self.pool)
        .await
    }

    /// A single post — but not if it has been taken down (hidden_at set).
    pub async fn get(&self, id: i64) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score
            FROM posts
            WHERE id = $1 AND hidden_at IS NULL
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Most recent visible posts first, capped by `limit` (the caller clamps it).
    pub async fn list_recent(&self, limit: i64) -> Result<Vec<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score
            FROM posts
            WHERE hidden_at IS NULL
            ORDER BY created_at DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Record a report of a post. Idempotent per (post, reporter) — re-reporting
    /// is a no-op (returns `false`), not amplification. A non-existent post hits
    /// the FK and surfaces as a 404 at the service.
    pub async fn report(
        &self,
        post_id: i64,
        reporter_id: i64,
        reason: &str,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            INSERT INTO post_reports (post_id, reporter_id, reason)
            VALUES ($1, $2, $3)
            ON CONFLICT (post_id, reporter_id) DO NOTHING
            "#,
            post_id,
            reporter_id,
            reason
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Take down (`hidden_at = now()`) or restore (`hidden_at = NULL`) a post.
    /// Returns the row, or `None` if no such post.
    pub async fn set_hidden(
        &self,
        id: i64,
        hidden_at: Option<DateTime<Utc>>,
    ) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            UPDATE posts SET hidden_at = $2
            WHERE id = $1
            RETURNING id, author_id, category, body, created_at, popularity_score
            "#,
            id,
            hidden_at
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Post ids with NO `content_signals` row yet (and not taken down), id-ordered,
    /// after a cursor, capped by `limit`. The backfill producer: the existing corpus
    /// is otherwise invisible to the ingestion pipeline, which only sees NEW posts.
    /// Read-only — it selects ids; it never touches the signals payload or the feed.
    pub async fn unanalyzed_post_ids(
        &self,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT p.id AS "id!"
            FROM posts p
            LEFT JOIN content_signals cs ON cs.post_id = p.id
            WHERE cs.post_id IS NULL
              AND p.hidden_at IS NULL
              AND p.id > $1
            ORDER BY p.id
            LIMIT $2
            "#,
            after_id,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }

    /// How many visible posts have NO `content_signals` row yet — i.e. exactly what
    /// a full backfill sweep would enqueue. Read-only count for operator status.
    pub async fn count_unanalyzed_posts(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM posts p
            LEFT JOIN content_signals cs ON cs.post_id = p.id
            WHERE cs.post_id IS NULL AND p.hidden_at IS NULL
            "#
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Count of analysed posts grouped by the `model_version` that produced them.
    /// Lets an operator watch a re-analysis sweep migrate the corpus from one model
    /// version to the next. Read-only — counts rows, never reads the signals payload.
    pub async fn signals_count_by_model_version(&self) -> Result<Vec<(String, i64)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT model_version, COUNT(*) AS "count!"
            FROM content_signals
            GROUP BY model_version
            ORDER BY model_version
            "#
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.model_version, r.count))
            .collect())
    }

    /// Reported posts with their report counts, most-reported first (operator
    /// review queue).
    pub async fn list_reported(&self, limit: i64) -> Result<Vec<ReportedPost>, sqlx::Error> {
        sqlx::query_as!(
            ReportedPost,
            r#"
            SELECT
                p.id AS "post_id!",
                COUNT(r.id) AS "report_count!",
                (p.hidden_at IS NOT NULL) AS "hidden!"
            FROM posts p
            JOIN post_reports r ON r.post_id = p.id
            GROUP BY p.id
            ORDER BY COUNT(r.id) DESC, p.id
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }
}
