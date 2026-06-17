//! Post business logic: validate and normalise input, and translate database
//! constraint violations into meaningful API errors.

use chrono::Utc;

use crate::error::ApiError;
use crate::posts::model::{NewPost, Post, ReportedPost};
use crate::posts::repository::PostRepository;
use crate::queue::IngestionQueue;
use db::PgPool;

/// Hard cap on how many posts one list request can return.
const MAX_LIST_LIMIT: i64 = 200;
/// Hard cap on a report reason's length.
const MAX_REASON_LEN: usize = 500;

#[derive(Clone)]
pub struct PostService {
    repo: PostRepository,
    /// Offers each new post to the AI ingestion pipeline. Optional so tests and
    /// callers that don't need ingestion can skip Redis entirely.
    ingestion: Option<IngestionQueue>,
}

impl PostService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repo: PostRepository::new(pool),
            ingestion: None,
        }
    }

    /// Wire the ingestion queue so created posts are offered to the pipeline.
    pub fn with_ingestion(pool: PgPool, ingestion: IngestionQueue) -> Self {
        Self {
            repo: PostRepository::new(pool),
            ingestion: Some(ingestion),
        }
    }

    pub async fn create(&self, mut new: NewPost) -> Result<Post, ApiError> {
        new.body = new.body.trim().to_string();
        if new.body.is_empty() {
            return Err(ApiError::Validation("empty_body"));
        }
        // Normalise category the same way users' declared categories are normalised.
        new.category = new
            .category
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty());

        let post = self.repo.create(&new).await.map_err(map_create_error)?;

        // Offer the new post to the AI ingestion pipeline. Best-effort: a Redis
        // hiccup must never fail a post (the post is the source of truth; the
        // pipeline can also backfill). Mirrors media finalize → transcode enqueue.
        if let Some(queue) = &self.ingestion {
            if let Err(err) = queue.enqueue(post.id).await {
                tracing::warn!(post_id = post.id, error = %err, "failed to enqueue ingestion job");
            }
        }

        Ok(post)
    }

    pub async fn get(&self, id: i64) -> Result<Post, ApiError> {
        self.repo.get(id).await?.ok_or(ApiError::NotFound)
    }

    pub async fn list_recent(&self, limit: i64) -> Result<Vec<Post>, ApiError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        Ok(self.repo.list_recent(limit).await?)
    }

    /// Record a user's report of a post. Idempotent per (post, reporter). 404 if
    /// the post does not exist.
    pub async fn report(
        &self,
        post_id: i64,
        reporter_id: i64,
        reason: String,
    ) -> Result<(), ApiError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(ApiError::Validation("empty_reason"));
        }
        if reason.len() > MAX_REASON_LEN {
            return Err(ApiError::Validation("reason_too_long"));
        }
        self.repo
            .report(post_id, reporter_id, reason)
            .await
            .map_err(map_report_error)?;
        Ok(())
    }

    /// Operator action: take a post down (hide it) or restore it. 404 if no such
    /// post. A hidden post drops out of the feed and public reads.
    pub async fn set_visibility(&self, post_id: i64, hidden: bool) -> Result<Post, ApiError> {
        let hidden_at = hidden.then(Utc::now);
        self.repo
            .set_hidden(post_id, hidden_at)
            .await?
            .ok_or(ApiError::NotFound)
    }

    /// Operator review queue: reported posts with their report counts.
    pub async fn list_reported(&self, limit: i64) -> Result<Vec<ReportedPost>, ApiError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        Ok(self.repo.list_reported(limit).await?)
    }
}

/// A report of a non-existent post hits the FK — a client error (404), not a fault.
fn map_report_error(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::NotFound;
        }
    }
    ApiError::Database(err)
}

/// A post for a non-existent author hits the FK constraint — that's a client
/// error (bad author), not a server fault, so surface it as a 400.
fn map_create_error(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::Validation("unknown_author");
        }
    }
    ApiError::Database(err)
}
