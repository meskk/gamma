//! Postgres-backed interaction repository — append-only writes plus an
//! epoch-scoped read for building the interaction graph.

use crate::interactions::model::InteractionEvent;
use db::PgPool;

#[derive(Clone)]
pub struct InteractionRepository {
    pool: PgPool,
}

impl InteractionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append one event. The column `type` is a SQL keyword, so it is aliased to
    /// `type_code` on the way out to match the struct field.
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
        sqlx::query_as!(
            InteractionEvent,
            r#"
            INSERT INTO interaction_events (actor_id, target_id, post_id, type, weight, epoch_k)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, actor_id, target_id, post_id, type AS type_code, weight, created_at, epoch_k
            "#,
            actor_id,
            target_id,
            post_id,
            type_code,
            weight,
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
}
