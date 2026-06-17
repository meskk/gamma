//! Postgres-backed interaction repository — append-only writes plus an
//! epoch-scoped read for building the interaction graph.

use crate::interactions::model::{EpochEdge, InteractionEvent};
use db::PgPool;

#[derive(Clone)]
pub struct InteractionRepository {
    pool: PgPool,
}

impl InteractionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append one event, idempotently within an epoch. The unique index
    /// (actor, type, epoch, target, post) means an identical repeat does NOT add
    /// weight — so a spammer can't inflate their edges by re-liking. A repeat
    /// returns the ALREADY-stored event rather than erroring. The column `type` is
    /// a SQL keyword, so it is aliased to `type_code` on the way out.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        actor_id: i64,
        type_code: i16,
        target_id: Option<i64>,
        post_id: Option<i64>,
        weight: f64,
        epoch_k: i32,
    ) -> Result<InteractionEvent, sqlx::Error> {
        let inserted = sqlx::query_as!(
            InteractionEvent,
            r#"
            INSERT INTO interaction_events (actor_id, target_id, post_id, type, weight, epoch_k)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT DO NOTHING
            RETURNING id, actor_id, target_id, post_id, type AS type_code, weight, created_at, epoch_k
            "#,
            actor_id,
            target_id,
            post_id,
            type_code,
            weight,
            epoch_k
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(event) = inserted {
            return Ok(event);
        }

        // The tuple already exists this epoch — return the original event so the
        // capture is idempotent (NULL target/post compared with IS NOT DISTINCT).
        sqlx::query_as!(
            InteractionEvent,
            r#"
            SELECT id, actor_id, target_id, post_id, type AS type_code, weight, created_at, epoch_k
            FROM interaction_events
            WHERE actor_id = $1 AND type = $4 AND epoch_k = $5
              AND target_id IS NOT DISTINCT FROM $2
              AND post_id IS NOT DISTINCT FROM $3
            "#,
            actor_id,
            target_id,
            post_id,
            type_code,
            epoch_k
        )
        .fetch_one(&self.pool)
        .await
    }

    /// All events stamped with the given epoch, in insertion order — the input to
    /// building the column-normalised matrix `M` for the node score.
    pub async fn list_by_epoch(&self, epoch_k: i32) -> Result<Vec<InteractionEvent>, sqlx::Error> {
        sqlx::query_as!(
            InteractionEvent,
            r#"
            SELECT id, actor_id, target_id, post_id, type AS type_code, weight, created_at, epoch_k
            FROM interaction_events
            WHERE epoch_k = $1
            ORDER BY id
            "#,
            epoch_k
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Resolved user→user edges for one epoch, ready for the gem engine. For a
    /// post interaction with no explicit target, the post's author is the target
    /// (LEFT JOIN on post_id). Events with no resolvable target and self-loops are
    /// dropped here so the graph layer gets clean edges.
    pub async fn edges_for_epoch(&self, epoch_k: i32) -> Result<Vec<EpochEdge>, sqlx::Error> {
        sqlx::query_as!(
            EpochEdge,
            r#"
            SELECT
                ie.actor_id AS "actor_id!",
                COALESCE(ie.target_id, p.author_id) AS "target_id!",
                ie.weight AS "weight!",
                ie.created_at AS "created_at!"
            FROM interaction_events ie
            LEFT JOIN posts p ON p.id = ie.post_id
            WHERE ie.epoch_k = $1
              AND COALESCE(ie.target_id, p.author_id) IS NOT NULL
              AND ie.actor_id <> COALESCE(ie.target_id, p.author_id)
            "#,
            epoch_k
        )
        .fetch_all(&self.pool)
        .await
    }
}
