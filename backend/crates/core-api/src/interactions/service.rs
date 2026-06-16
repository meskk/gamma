//! Interaction capture logic: derive the edge weight from the type and stamp the
//! current epoch, then append. Stamping happens here (not in the client) so the
//! epoch is authoritative and consistent.

use chrono::Utc;
use domain::Epoch;

use crate::error::ApiError;
use crate::interactions::model::{InteractionEvent, NewInteraction};
use crate::interactions::repository::InteractionRepository;
use db::PgPool;

#[derive(Clone)]
pub struct InteractionService {
    repo: InteractionRepository,
}

impl InteractionService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: InteractionRepository::new(pool),
        }
    }

    pub async fn record(&self, new: NewInteraction) -> Result<InteractionEvent, ApiError> {
        let epoch = Epoch::from_unix_seconds(Utc::now().timestamp());
        let weight = new.r#type.weight();
        Ok(self
            .repo
            .record(
                new.actor_id,
                new.r#type.code(),
                new.target_id,
                new.post_id,
                weight,
                epoch.0 as i32,
            )
            .await?)
    }

    pub async fn list_by_epoch(&self, epoch_k: i32) -> Result<Vec<InteractionEvent>, ApiError> {
        Ok(self.repo.list_by_epoch(epoch_k).await?)
    }
}
