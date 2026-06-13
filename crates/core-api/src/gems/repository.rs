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

    /// Claim an epoch for settlement. Returns `true` if THIS call claimed it
    /// (first time), `false` if it was already settled. The `ON CONFLICT DO
    /// NOTHING` makes the claim atomic, so two concurrent workers can't both mint.
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
