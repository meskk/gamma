//! Postgres-backed user repository — the ONLY place that knows users SQL.
//!
//! Concrete (not a trait) on purpose: there is a single backing (Postgres) and
//! tests run against a real DB via `#[sqlx::test]`, so a mock-enabling trait would
//! be premature abstraction. Extract a trait later only if a second backing
//! actually appears. Queries use `query_as!` so they are checked against the live
//! schema at compile time (the `.sqlx` cache lets CI do this without a database).

use crate::interactions::model::InteractionType;
use crate::users::model::{NewUser, ReferralTerms, User};
use db::PgPool;

#[derive(Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, new: &NewUser) -> Result<User, sqlx::Error> {
        // A brand-new user has no posts and no followers — literals.
        sqlx::query_as!(
            User,
            r#"
            INSERT INTO users (declared_categories, bot_gate_v)
            VALUES ($1, $2)
            RETURNING id, created_at, declared_categories, bot_gate_v,
                      0::bigint AS "likes_received!", 0::bigint AS "followers_count!"
            "#,
            &new.declared_categories,
            new.bot_gate_v
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn get(&self, id: i64) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"
            SELECT id, created_at, declared_categories, bot_gate_v,
                   (SELECT COUNT(DISTINCT (ie.actor_id, ie.post_id)) FROM interaction_events ie
                    JOIN posts p ON p.id = ie.post_id
                    WHERE p.author_id = users.id AND ie.type = $2
                      AND ie.retracted_at IS NULL
                      AND p.hidden_at IS NULL AND p.area = 'public')
                       AS "likes_received!",
                   (SELECT COUNT(*) FROM follows f WHERE f.followee_id = users.id)
                       AS "followers_count!"
            FROM users
            WHERE id = $1
            "#,
            id,
            InteractionType::Like.code()
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Set a user's bot-gate (verified) flag. Returns the updated row, or `None`
    /// if no such user exists. Operator-only at the HTTP layer.
    pub async fn set_bot_gate(&self, id: i64, verified: bool) -> Result<Option<User>, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"
            UPDATE users SET bot_gate_v = $2
            WHERE id = $1
            RETURNING id, created_at, declared_categories, bot_gate_v,
                      (SELECT COUNT(DISTINCT (ie.actor_id, ie.post_id)) FROM interaction_events ie
                       JOIN posts p ON p.id = ie.post_id
                       WHERE p.author_id = users.id AND ie.type = $3
                         AND ie.retracted_at IS NULL
                         AND p.hidden_at IS NULL AND p.area = 'public')
                          AS "likes_received!",
                      (SELECT COUNT(*) FROM follows f WHERE f.followee_id = users.id)
                          AS "followers_count!"
            "#,
            id,
            verified,
            InteractionType::Like.code()
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Upsert a creator's referral contract (operator-only at the HTTP layer).
    /// The FK to `users` rejects contracts for nonexistent users — the caller
    /// maps that violation to 404.
    pub async fn upsert_referral_terms(
        &self,
        referrer_id: i64,
        bps: i32,
        duration_epochs: i64,
        note: Option<&str>,
    ) -> Result<ReferralTerms, sqlx::Error> {
        sqlx::query_as!(
            ReferralTerms,
            r#"
            INSERT INTO referral_terms (referrer_id, bps, duration_epochs, note)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (referrer_id) DO UPDATE
            SET bps = $2, duration_epochs = $3, note = $4, updated_at = now()
            RETURNING referrer_id, bps, duration_epochs, note
            "#,
            referrer_id,
            bps,
            duration_epochs,
            note
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Bot-gate (verified) flags for the given user ids — the gate `v_i` that the
    /// gem engine applies. Missing ids simply don't appear in the result.
    pub async fn verified_flags(&self, ids: &[i64]) -> Result<Vec<(i64, bool)>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT id, bot_gate_v FROM users WHERE id = ANY($1)"#,
            ids
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| (r.id, r.bot_gate_v)).collect())
    }
}
