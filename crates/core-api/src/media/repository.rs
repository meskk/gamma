//! Postgres-backed media repository — the only place that knows media SQL,
//! including the atomic paid-unlock transaction.

use crate::media::model::MediaAsset;
use db::PgPool;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UnlockError {
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error("insufficient gems")]
    InsufficientFunds,
}

/// Outcome of the unlock transaction.
pub struct UnlockOutcome {
    pub already_unlocked: bool,
}

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
        unlock_price: i64,
    ) -> Result<MediaAsset, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            INSERT INTO media_assets (owner_id, kind, object_key, content_type, status, unlock_price)
            VALUES ($1, $2, $3, $4, 'pending', $5)
            RETURNING id, owner_id, kind, object_key, content_type, status,
                      size_bytes, hls_manifest_key, transcode_status, unlock_price, created_at
            "#,
            owner_id,
            kind,
            object_key,
            content_type,
            unlock_price
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn get(&self, id: i64) -> Result<Option<MediaAsset>, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            SELECT id, owner_id, kind, object_key, content_type, status,
                   size_bytes, hls_manifest_key, transcode_status, unlock_price, created_at
            FROM media_assets
            WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn mark_ready(&self, id: i64, size_bytes: i64) -> Result<MediaAsset, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            UPDATE media_assets
            SET status = 'ready', size_bytes = $2
            WHERE id = $1
            RETURNING id, owner_id, kind, object_key, content_type, status,
                      size_bytes, hls_manifest_key, transcode_status, unlock_price, created_at
            "#,
            id,
            size_bytes
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn set_transcode_status(&self, id: i64, status: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE media_assets SET transcode_status = $2 WHERE id = $1",
            id,
            status
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_hls(&self, id: i64, manifest_key: &str) -> Result<MediaAsset, sqlx::Error> {
        sqlx::query_as!(
            MediaAsset,
            r#"
            UPDATE media_assets
            SET hls_manifest_key = $2, transcode_status = 'done'
            WHERE id = $1
            RETURNING id, owner_id, kind, object_key, content_type, status,
                      size_bytes, hls_manifest_key, transcode_status, unlock_price, created_at
            "#,
            id,
            manifest_key
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Is `viewer` entitled to `asset` via a recorded unlock?
    pub async fn is_unlocked(&self, viewer_id: i64, asset_id: i64) -> Result<bool, sqlx::Error> {
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                SELECT 1 FROM media_unlocks WHERE viewer_id = $1 AND asset_id = $2
            ) AS "exists!""#,
            viewer_id,
            asset_id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Pay to unlock, atomically (Phase 1a off-chain content payment): record the
    /// entitlement, debit the viewer the full price, credit the creator and the
    /// company, and let the burn amount be destroyed (not credited anywhere).
    ///
    /// Entitlement is claimed first, so a repeat unlock is a no-charge no-op. Any
    /// failure rolls the whole thing back — the viewer never loses gems without
    /// gaining access. Phase 1b moves this to on-chain transfers.
    #[allow(clippy::too_many_arguments)]
    pub async fn unlock(
        &self,
        viewer_id: i64,
        creator_id: i64,
        company_id: i64,
        asset_id: i64,
        price: i64,
        creator_amount: i64,
        company_fee: i64,
    ) -> Result<UnlockOutcome, UnlockError> {
        let mut tx = self.pool.begin().await?;

        // Claim entitlement first; if it already existed, no charge.
        let claimed = sqlx::query!(
            r#"INSERT INTO media_unlocks (viewer_id, asset_id)
               VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
            viewer_id,
            asset_id
        )
        .execute(&mut *tx)
        .await?;
        if claimed.rows_affected() == 0 {
            return Ok(UnlockOutcome {
                already_unlocked: true,
            });
        }

        // Debit the full price from the viewer (guarded → insufficient funds).
        let debited = sqlx::query!(
            r#"UPDATE gem_balances SET balance = balance - $2, updated_at = now()
               WHERE user_id = $1 AND balance >= $2"#,
            viewer_id,
            price
        )
        .execute(&mut *tx)
        .await?;
        if debited.rows_affected() == 0 {
            return Err(UnlockError::InsufficientFunds);
        }

        // Credit creator and company; the remaining (burn) amount is destroyed.
        for (account, amount) in [(creator_id, creator_amount), (company_id, company_fee)] {
            if amount > 0 {
                sqlx::query!(
                    r#"INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
                       ON CONFLICT (user_id) DO UPDATE
                       SET balance = gem_balances.balance + EXCLUDED.balance, updated_at = now()"#,
                    account,
                    amount
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(UnlockOutcome {
            already_unlocked: false,
        })
    }
}
