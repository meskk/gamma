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

    /// Insert a comment. A comment for a non-existent post hits the FK constraint,
    /// surfaced as a 404 by the service.
    pub async fn create(
        &self,
        post_id: i64,
        author_id: i64,
        body: &str,
    ) -> Result<Comment, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            INSERT INTO comments (post_id, author_id, body)
            VALUES ($1, $2, $3)
            RETURNING id, post_id, author_id, body, created_at
            "#,
            post_id,
            author_id,
            body
        )
        .fetch_one(&self.pool)
        .await
    }

    /// A post's comments, oldest first.
    pub async fn list_for_post(&self, post_id: i64) -> Result<Vec<Comment>, sqlx::Error> {
        sqlx::query_as!(
            Comment,
            r#"
            SELECT id, post_id, author_id, body, created_at
            FROM comments
            WHERE post_id = $1
            ORDER BY created_at ASC
            "#,
            post_id
        )
        .fetch_all(&self.pool)
        .await
    }
}
