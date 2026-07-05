//! Interaction capture logic: derive the edge weight from the type and stamp the
//! current epoch, then append. Stamping happens here (not in the client) so the
//! epoch is authoritative and consistent.

use chrono::Utc;
use domain::Epoch;
use econ_params::EconParams;

use crate::error::ApiError;
use crate::interactions::model::{InteractionEvent, NewInteraction};
use crate::interactions::repository::InteractionRepository;
use db::PgPool;

#[derive(Clone)]
pub struct InteractionService {
    repo: InteractionRepository,
    econ: EconParams,
}

impl InteractionService {
    pub fn new(pool: PgPool) -> Self {
        Self::with_econ(pool, EconParams::default())
    }

    pub fn with_econ(pool: PgPool, econ: EconParams) -> Self {
        Self {
            repo: InteractionRepository::new(pool),
            econ,
        }
    }

    pub async fn record(&self, new: NewInteraction) -> Result<InteractionEvent, ApiError> {
        let epoch = Epoch::from_unix_seconds(Utc::now().timestamp());
        let weight = new.r#type.weight(&self.econ.interaction_weights);

        // Normalise the identifiers so the dedup index actually binds to the edge it
        // guards. The edge target is resolved as COALESCE(target_id, post.author):
        // when `target_id` is set it WINS and `post_id` is irrelevant to the
        // resulting edge — yet `post_id` is part of the dedup key
        // (actor, type, epoch, target_id, post_id). Left unconstrained, an attacker
        // records N rows {target_id: X, post_id: <N distinct posts>}: each is a
        // distinct dedup tuple (all survive) but all resolve to the SAME edge
        // actor→X, additively inflating X's weight and defeating the 0009 index.
        // Fix: if `target_id` is set, clear `post_id` before it reaches the key, so
        // the tuple collapses to one edge per (actor, type, target) per epoch. A
        // post interaction (no explicit target) keeps `post_id` and resolves to the
        // author. At least one identifier must be present.
        let (target_id, post_id) = match (new.target_id, new.post_id) {
            (Some(t), _) => (Some(t), None),
            (None, Some(p)) => (None, Some(p)),
            (None, None) => {
                return Err(ApiError::Validation("interaction_requires_target_or_post"))
            }
        };

        // A target user or post that doesn't exist trips a foreign key (migration
        // 0015) — that's a client error (the thing being interacted with is gone),
        // so surface it as 404 rather than a 500. The actor comes from the session,
        // so its FK can't realistically fire.
        self.repo
            .record(
                new.actor_id,
                new.r#type.code(),
                target_id,
                post_id,
                weight,
                epoch.0 as i32,
            )
            .await
            .map_err(|e| ApiError::on_fk_violation(e, ApiError::NotFound))
    }

    pub async fn list_by_epoch(&self, epoch_k: i32) -> Result<Vec<InteractionEvent>, ApiError> {
        Ok(self.repo.list_by_epoch(epoch_k).await?)
    }
}
