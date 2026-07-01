//! Postgres-backed follow repository — the only place that knows follows SQL.

use crate::follows::model::Follow;
use db::PgPool;

#[derive(Clone)]
pub struct FollowRepository {
    pool: PgPool,
}

impl FollowRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Idempotent: following an already-followed account is a no-op (the PK
    /// makes the edge unique), so callers never see a duplicate-key error.
    pub async fn follow(&self, follower: i64, followee: i64) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO follows (follower_id, followee_id)
            VALUES ($1, $2)
            ON CONFLICT (follower_id, followee_id) DO NOTHING
            "#,
            follower,
            followee
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Idempotent: unfollowing a non-edge simply affects zero rows.
    pub async fn unfollow(&self, follower: i64, followee: i64) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"DELETE FROM follows WHERE follower_id = $1 AND followee_id = $2"#,
            follower,
            followee
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The accounts `follower` follows, newest edge first, bounded by
    /// `limit`/`offset` so the list can't return an unbounded result set.
    pub async fn list_following(
        &self,
        follower: i64,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Follow>, sqlx::Error> {
        sqlx::query_as!(
            Follow,
            r#"
            SELECT follower_id, followee_id, created_at
            FROM follows
            WHERE follower_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            follower,
            limit,
            offset
        )
        .fetch_all(&self.pool)
        .await
    }
}
