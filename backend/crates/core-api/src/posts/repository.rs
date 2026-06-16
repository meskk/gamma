//! Postgres-backed post repository — the only place that knows posts SQL.
//! Same shape as the users repository (concrete struct, `query_as!` checked
//! queries). Adds `list_recent` to show the multi-row (`fetch_all`) template.

use crate::posts::model::{NewPost, Post};
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

    pub async fn get(&self, id: i64) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score
            FROM posts
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Most recent posts first, capped by `limit` (the caller clamps the bound).
    pub async fn list_recent(&self, limit: i64) -> Result<Vec<Post>, sqlx::Error> {
        sqlx::query_as!(
            Post,
            r#"
            SELECT id, author_id, category, body, created_at, popularity_score
            FROM posts
            ORDER BY created_at DESC
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }
}
