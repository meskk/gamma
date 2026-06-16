//! Post business logic: validate and normalise input, and translate database
//! constraint violations into meaningful API errors.

use crate::error::ApiError;
use crate::posts::model::{NewPost, Post};
use crate::posts::repository::PostRepository;
use crate::queue::IngestionQueue;
use db::PgPool;

/// Hard cap on how many posts one list request can return.
const MAX_LIST_LIMIT: i64 = 200;

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
