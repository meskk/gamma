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

    /// Does the private-area invariant allow `viewer` to reach `asset` (P-4/A4e,
    /// ADR 0011 §5)? This is the media rail's answer to the one path `hidden_at`
    /// structurally misses: media entitlement is per-asset and knows no posts, so a
    /// private post's price-0 asset would otherwise be presign-fetchable by anyone.
    ///
    /// The link is `posts.media_id -> media_assets.id` (nullable, NON-unique, so an
    /// asset may back several posts). Allowed iff the asset is attached to NO post
    /// at all (unattached assets are out of P-4 scope — behaviour unchanged) OR to
    /// at least one VISIBLE post the viewer may see (`hidden_at IS NULL` AND the
    /// canonical area predicate). This "public rescue" is deliberate: if the same
    /// asset also sits on a public/entitled/free post, its bytes are already
    /// legitimately reachable via that post, so serving them is not a leak —
    /// fail-closed fires only when EVERY attached post is hidden or
    /// private-and-unentitled. `viewer` is always an authenticated user on the media
    /// routes, so the free arm needs no `IS NOT NULL` guard.
    ///
    /// The `hidden_at IS NULL` conjunct lives on the RESCUE arm ONLY — a taken-down
    /// post no longer rescues its asset, so moderator-removed media stops streaming
    /// to enumerating non-owners (consistent with every text read path). It does
    /// NOT go on the unattached `NOT EXISTS` arm: an attached-but-hidden post must
    /// still count as "attached" (else the asset would fall through to the
    /// unattached=allowed branch). RESIDUAL (tracked follow-up): the service-layer
    /// owner short-circuit still lets the OWNER reach their own taken-down media,
    /// and there is no asset-level takedown for the one-asset-many-posts case.
    pub async fn media_area_allows(
        &self,
        asset_id: i64,
        viewer_id: i64,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar!(
            r#"
            SELECT (
                NOT EXISTS (SELECT 1 FROM posts p WHERE p.media_id = $1)
                OR EXISTS (
                    SELECT 1 FROM posts p
                    WHERE p.media_id = $1 AND p.hidden_at IS NULL
                      AND (
                        p.area = 'public'
                        OR p.author_id = $2
                        OR EXISTS (SELECT 1 FROM area_entitlements ae WHERE ae.viewer_id = $2 AND ae.creator_id = p.author_id AND (ae.expires_at IS NULL OR ae.expires_at > now()))
                        OR EXISTS (SELECT 1 FROM private_areas pa WHERE pa.creator_id = p.author_id AND pa.access_model = 'free')
                      )
                )
            ) AS "allowed!"
            "#,
            asset_id,
            viewer_id
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Pay to unlock, atomically (Phase 1a off-chain content payment): record the
    /// entitlement, debit the viewer the full price, credit the creator and the
    /// company, and record the burned remainder as a destruction.
    ///
    /// All money movement goes through the ledger crate's transaction-scoped
    /// primitives (debit/credit/burn), so `gem_balances` mutation lives in the
    /// ledger seam — not hand-written here — and every leg (including the burn) is
    /// journaled in `ledger_entries`. Entitlement is claimed first, so a repeat
    /// unlock is a no-charge no-op; any failure rolls the whole transaction back,
    /// so the viewer never loses gems without gaining access. Phase 1b swaps the
    /// ledger backing with no change to this composition.
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
        burned: i64,
        epoch_k: i64,
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
        if !ledger::debit_tx(&mut tx, viewer_id, price, epoch_k, "unlock_debit", asset_id).await? {
            return Err(UnlockError::InsufficientFunds);
        }
        // Credit creator and company; record the destroyed remainder as a burn.
        if creator_amount > 0 {
            ledger::credit_tx(
                &mut tx,
                creator_id,
                creator_amount,
                epoch_k,
                "unlock_credit",
                asset_id,
            )
            .await?;
        }
        if company_fee > 0 {
            ledger::credit_tx(
                &mut tx,
                company_id,
                company_fee,
                epoch_k,
                "unlock_credit",
                asset_id,
            )
            .await?;
        }
        if burned > 0 {
            ledger::burn_tx(&mut tx, burned, epoch_k, asset_id).await?;
        }

        tx.commit().await?;
        Ok(UnlockOutcome {
            already_unlocked: false,
        })
    }
}
