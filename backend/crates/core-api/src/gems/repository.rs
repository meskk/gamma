//! Settlement-marker persistence — the `epoch_settlements` idempotency guard.

use db::PgPool;

#[derive(Clone)]
pub struct GemRepository {
    pool: PgPool,
}

impl GemRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Whether this epoch has already been settled (the marker row exists). The
    /// fast path: skip recomputing/minting an epoch that is already done.
    pub async fn is_settled(&self, epoch_k: i64) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM epoch_settlements WHERE epoch_k = $1) AS "exists!""#,
            epoch_k
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Record that an epoch has been settled. Returns `true` if THIS call recorded
    /// it (first time), `false` if a marker already existed. Written AFTER minting,
    /// so the marker can never flag an under-paid epoch as done.
    pub async fn claim_epoch(
        &self,
        epoch_k: i64,
        emission: i64,
        user_count: i32,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            INSERT INTO epoch_settlements (epoch_k, emission, user_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (epoch_k) DO NOTHING
            "#,
            epoch_k,
            emission,
            user_count
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() == 1)
    }
}
