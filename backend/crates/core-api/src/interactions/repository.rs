//! Postgres-backed interaction repository — append-only writes (plus the ADR 0012
//! retraction, which VOIDS rows in place rather than deleting them) and an
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
    /// (actor, type, epoch, target, post, comment) means an identical repeat does
    /// NOT add weight — so a spammer can't inflate their edges by re-liking. A
    /// repeat returns the ALREADY-stored event (original weight) rather than
    /// erroring; if that event was retracted (un-liked), the repeat UN-VOIDS it —
    /// a like → un-like → like cycle within one epoch restores the single
    /// original row instead of minting a second one. The column `type` is a SQL
    /// keyword, so it is aliased to `type_code` on the way out.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        &self,
        actor_id: i64,
        type_code: i16,
        target_id: Option<i64>,
        post_id: Option<i64>,
        comment_id: Option<i64>,
        weight: f64,
        epoch_k: i32,
    ) -> Result<InteractionEvent, sqlx::Error> {
        sqlx::query_as!(
            InteractionEvent,
            r#"
            INSERT INTO interaction_events (actor_id, target_id, post_id, comment_id, type, weight, epoch_k)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (actor_id, type, epoch_k, target_id, post_id, comment_id)
            DO UPDATE SET retracted_at = NULL
            RETURNING id, actor_id, target_id, post_id, comment_id,
                      type AS type_code, weight, created_at, epoch_k, retracted_at
            "#,
            actor_id,
            target_id,
            post_id,
            comment_id,
            type_code,
            weight,
            epoch_k
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Void (`retracted_at = now()`) every ACTIVE event matching the canonical
    /// tuple, across ALL epochs — the product-level un-like (ADR 0012). Rows are
    /// never deleted: the journal keeps the history, settlement and the read-side
    /// counts skip voided rows. Voiding rows from an already-settled epoch is
    /// deliberate and display-only — settlement read its edges at settle time and
    /// is idempotent per epoch, so a past payout is never re-opened. Returns how
    /// many rows were voided (0 = nothing was liked; the caller treats that as
    /// success — idempotent).
    pub async fn retract(
        &self,
        actor_id: i64,
        type_code: i16,
        target_id: Option<i64>,
        post_id: Option<i64>,
        comment_id: Option<i64>,
    ) -> Result<u64, sqlx::Error> {
        let res = sqlx::query!(
            r#"
            UPDATE interaction_events SET retracted_at = now()
            WHERE actor_id = $1 AND type = $2 AND retracted_at IS NULL
              AND target_id IS NOT DISTINCT FROM $3
              AND post_id IS NOT DISTINCT FROM $4
              AND comment_id IS NOT DISTINCT FROM $5
            "#,
            actor_id,
            type_code,
            target_id,
            post_id,
            comment_id
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// All events stamped with the given epoch, in insertion order — the RAW
    /// journal, including retracted (voided) rows: this is the audit read, not an
    /// economic one. Everything economic goes through `edges_for_epoch`, which
    /// skips voided rows.
    pub async fn list_by_epoch(&self, epoch_k: i32) -> Result<Vec<InteractionEvent>, sqlx::Error> {
        sqlx::query_as!(
            InteractionEvent,
            r#"
            SELECT id, actor_id, target_id, post_id, comment_id,
                   type AS type_code, weight, created_at, epoch_k, retracted_at
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
    /// post interaction with no explicit target, the post's author is the target;
    /// for a comment interaction, the COMMENT's author (LEFT JOINs). Events with
    /// no resolvable target, self-loops, and RETRACTED (un-liked, ADR 0012) events
    /// are dropped here so the graph layer gets clean edges.
    ///
    /// A post interaction whose target is the author of a TAKEN-DOWN post
    /// (`hidden_at` set) is also dropped: moderation must stop removed content from
    /// conferring social weight — including likes recorded before the takedown. The
    /// same drop applies to a PRIVATE post (`area = 'private'`, P-4/A4): engagement
    /// behind the paywall never feeds the settlement graph (ADR 0011 §5 — Rail-1 vs
    /// Rail-2 separation, and no bot-harvest surface hidden from report-driven
    /// moderation). A comment-directed event is gated by ITS post's visibility the
    /// same way (a like on a comment under a taken-down or private post confers
    /// nothing). All three arms live INSIDE the `target_id IS NULL` branch: a
    /// direct user→user interaction (explicit `target_id`) is kept regardless of
    /// any attached content's visibility, since its weight does not flow from the
    /// content — putting the `area` filter at the top level would null-drop every
    /// direct edge via the LEFT JOINs and zero all tip/direct gems.
    pub async fn edges_for_epoch(&self, epoch_k: i32) -> Result<Vec<EpochEdge>, sqlx::Error> {
        sqlx::query_as!(
            EpochEdge,
            r#"
            SELECT
                ie.actor_id AS "actor_id!",
                COALESCE(ie.target_id, p.author_id, c.author_id) AS "target_id!",
                ie.weight AS "weight!",
                ie.created_at AS "created_at!"
            FROM interaction_events ie
            LEFT JOIN posts p ON p.id = ie.post_id
            LEFT JOIN comments c ON c.id = ie.comment_id
            LEFT JOIN posts cp ON cp.id = c.post_id
            WHERE ie.epoch_k = $1
              AND ie.retracted_at IS NULL
              AND COALESCE(ie.target_id, p.author_id, c.author_id) IS NOT NULL
              AND ie.actor_id <> COALESCE(ie.target_id, p.author_id, c.author_id)
              AND (ie.target_id IS NOT NULL
                   OR (ie.post_id IS NOT NULL AND p.hidden_at IS NULL AND p.area = 'public')
                   OR (ie.comment_id IS NOT NULL AND cp.hidden_at IS NULL AND cp.area = 'public'))
            "#,
            epoch_k
        )
        .fetch_all(&self.pool)
        .await
    }
}
