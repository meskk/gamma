//! Content-signal business logic: validate the write-back and map a write-back
//! for an unknown post to a client error.

use serde_json::Value;

use crate::error::ApiError;
use crate::signals::model::ContentSignal;
use crate::signals::repository::ContentSignalRepository;
use db::PgPool;

#[derive(Clone)]
pub struct SignalService {
    repo: ContentSignalRepository,
}

impl SignalService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: ContentSignalRepository::new(pool),
        }
    }

    /// Record the pipeline's signals for a post (upsert).
    pub async fn record(
        &self,
        post_id: i64,
        model_version: String,
        signals: Value,
    ) -> Result<(), ApiError> {
        let model_version = model_version.trim();
        if model_version.is_empty() {
            return Err(ApiError::Validation("empty_model_version"));
        }
        self.repo
            .upsert(post_id, model_version, &signals)
            .await
            .map_err(map_fk)
    }

    /// The stored signals for a post, or 404 if none yet.
    pub async fn get(&self, post_id: i64) -> Result<ContentSignal, ApiError> {
        self.repo.get(post_id).await?.ok_or(ApiError::NotFound)
    }
}

/// A write-back for a non-existent post hits the FK — a client error, not a fault.
fn map_fk(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::Validation("unknown_post");
        }
    }
    ApiError::Database(err)
}
