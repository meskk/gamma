//! Postgres persistence for comments — the only place that knows comments SQL.

use crate::comments::model::Comment;
use db::PgPool;

#[derive(Clone)]
pub struct CommentRepository {
    pool: PgPool,
}

impl CommentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a comment — but only when the target post exists AND is visible
    /// (not taken down). Returns `None` when the post is missing or hidden, which
    /// the service surfaces as a 404. A non-existent author still hits the FK.
    pub async fn create(
        &self,
        post_id: i64,
        author_id: i64,
        body: &str,
    ) -> Result<Option<Comment>, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            INSERT INTO comments (post_id, author_id, body)
            SELECT $1, $2, $3
            WHERE EXISTS (SELECT 1 FROM posts WHERE id = $1 AND hidden_at IS NULL)
            RETURNING id, post_id, author_id, body, created_at
            "#,
            post_id,
            author_id,
            body
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// A visible post's comments, oldest first, bounded by `limit`/`offset` so a
    /// pathological thread can't return an unbounded result set. Comments on a
    /// taken-down (`hidden_at` set) post are excluded — moderation hides the thread.
    pub async fn list_for_post(
        &self,
        post_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Comment>, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            SELECT c.id, c.post_id, c.author_id, c.body, c.created_at
            FROM comments c
            JOIN posts p ON p.id = c.post_id
            WHERE c.post_id = $1 AND p.hidden_at IS NULL
            ORDER BY c.created_at ASC
            LIMIT $2 OFFSET $3
            "#,
            post_id,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
    }
}
