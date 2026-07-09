//! Postgres persistence for the private area — the only place that knows the
//! `private_areas` / `area_entitlements` / `purchases` SQL.

use chrono::{DateTime, Utc};

use crate::private_area::model::{AccessModel, EntitlementSource, NewPurchase, PrivateArea};
use db::PgPool;

#[derive(Clone)]
pub struct PrivateAreaRepository {
    pool: PgPool,
}

impl PrivateAreaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create or update a creator's area configuration (one row per creator).
    pub async fn upsert_area(
        &self,
        creator_id: i64,
        access_model: AccessModel,
        price_cents: i64,
        description: &str,
    ) -> Result<PrivateArea, sqlx::Error> {
        sqlx::query_as!(
            PrivateArea,
            r#"
            INSERT INTO private_areas (creator_id, access_model, price_cents, description, updated_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (creator_id) DO UPDATE
            SET access_model = EXCLUDED.access_model,
                price_cents = EXCLUDED.price_cents,
                description = EXCLUDED.description,
                updated_at = now()
            RETURNING creator_id, access_model, price_cents, currency, description, updated_at
            "#,
            creator_id,
            access_model.as_str(),
            price_cents,
            description
        )
        .fetch_one(&self.pool)
        .await
    }

    /// A creator's area configuration, if they ever set one up.
    pub async fn get_area(&self, creator_id: i64) -> Result<Option<PrivateArea>, sqlx::Error> {
        sqlx::query_as!(
            PrivateArea,
            r#"
            SELECT creator_id, access_model, price_cents, currency, description, updated_at
            FROM private_areas
            WHERE creator_id = $1
            "#,
            creator_id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Grant (or refresh) a viewer's access to a creator's area. A grant only
    /// ever EXTENDS access (NULL expiry = permanent, ordered as +infinity):
    /// `invoice.paid` pushes `expires_at` forward, a one-time purchase writes
    /// NULL — and a stale or out-of-order provider event (Stripe retries for
    /// days, unordered) can neither rewind an expiry nor downgrade a
    /// permanent, paid-forever entitlement to a lapsing one. Lapse revokes by
    /// itself (see `is_entitled`), no cron; deliberate revocation (refund,
    /// operator removal) becomes its own explicit path with A6 — never a side
    /// effect of a grant.
    pub async fn grant_entitlement(
        &self,
        viewer_id: i64,
        creator_id: i64,
        source: EntitlementSource,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO area_entitlements (viewer_id, creator_id, source, expires_at, granted_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (viewer_id, creator_id) DO UPDATE
            SET source = EXCLUDED.source,
                expires_at = EXCLUDED.expires_at,
                granted_at = now()
            WHERE area_entitlements.expires_at IS NOT NULL
              AND (EXCLUDED.expires_at IS NULL
                   OR EXCLUDED.expires_at > area_entitlements.expires_at)
            "#,
            viewer_id,
            creator_id,
            source.as_str(),
            expires_at
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Does the viewer hold a live entitlement ROW for the creator's area?
    /// The creator is always entitled to their own area; an expired row
    /// answers false without any cleanup job. NOT the full visibility
    /// predicate: a 'free' area grants access without any entitlement row —
    /// A4's check is (creator OR entitled OR area is free) and must consult
    /// `private_areas.access_model` too.
    pub async fn is_entitled(&self, viewer_id: i64, creator_id: i64) -> Result<bool, sqlx::Error> {
        if viewer_id == creator_id {
            return Ok(true);
        }
        sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM area_entitlements
                WHERE viewer_id = $1 AND creator_id = $2
                  AND (expires_at IS NULL OR expires_at > now())
            ) AS "entitled!"
            "#,
            viewer_id,
            creator_id
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Record a provider-side purchase in the audit mirror. Returns whether
    /// the row is NEW — a replayed webhook (same provider_ref) records nothing
    /// twice and must also not re-trigger side effects at the caller.
    pub async fn record_purchase(&self, purchase: NewPurchase<'_>) -> Result<bool, sqlx::Error> {
        let inserted = sqlx::query!(
            r#"
            INSERT INTO purchases
                (provider, provider_ref, viewer_id, creator_id, kind,
                 amount_cents, currency, fee_cents, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (provider, provider_ref) DO NOTHING
            "#,
            purchase.provider,
            purchase.provider_ref,
            purchase.viewer_id,
            purchase.creator_id,
            purchase.kind,
            purchase.amount_cents,
            purchase.currency,
            purchase.fee_cents,
            purchase.status
        )
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(inserted == 1)
    }
}
