//! Post business logic: validate and normalise input, and translate database
//! constraint violations into meaningful API errors.

use std::collections::BTreeMap;

use chrono::Utc;
use serde::Serialize;
use ts_rs::TS;

use crate::error::ApiError;
use crate::posts::model::{NewPost, Post, ReportedPost};
use crate::posts::repository::PostRepository;
use crate::queue::IngestionQueue;
use db::PgPool;

/// Hard cap on how many posts one list request can return.
const MAX_LIST_LIMIT: i64 = 200;
/// Hard cap on a report reason's length.
const MAX_REASON_LEN: usize = 500;
/// Hard cap on how many posts one backfill page enqueues (the operator paginates).
const MAX_BACKFILL_LIMIT: i64 = 1000;

/// Result of one backfill page: how many ids were enqueued, and the cursor to
/// resume from (`?after=`). `enqueued == 0` means the sweep is drained.
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct BackfillResult {
    pub enqueued: i64,
    pub last_id: i64,
}

/// A snapshot of how far ingestion analysis has progressed over the corpus. Pure
/// observability for an operator sizing a backfill or watching a re-analysis sweep.
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct IngestionStatus {
    /// Visible posts that have a `content_signals` row.
    pub analyzed: i64,
    /// Visible posts with no signals row yet (what a full backfill would enqueue).
    pub unanalyzed: i64,
    /// Analysed-post counts keyed by the model version that produced them.
    pub by_model_version: BTreeMap<String, i64>,
}

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

        // Pre-validate any attached media: it must exist AND belong to the author.
        // This keeps the insert from ever tripping the post's media FK, so a missing
        // or not-owned asset is reported precisely as `unknown_media` rather than
        // being misattributed to a bad author. It also blocks attaching someone
        // else's asset.
        if let Some(media_id) = new.media_id {
            if !self.repo.media_owned_by(media_id, new.author_id).await? {
                return Err(ApiError::Validation("unknown_media"));
            }
        }

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

    /// Recent visible posts; when `author_id` is `Some`, only that author's (a
    /// profile feed).
    pub async fn list(
        &self,
        author_id: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Post>, ApiError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let offset = offset.max(0);
        Ok(self.repo.list(author_id, limit, offset).await?)
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

    /// Enqueue a page of not-yet-analysed posts so the ingestion pipeline can sweep
    /// the existing corpus — which it otherwise never sees, since `create` is the
    /// only other producer. ENQUEUE-ONLY: it never writes `content_signals` and
    /// never reads signal contents, so no signal shape is touched and the API still
    /// owns the database (ADR 0006). Safe to repeat: already-analysed posts are
    /// filtered out and duplicate enqueues are harmless (the consumer upserts).
    /// Paginate with the returned `last_id` (as `after`) until `enqueued == 0`.
    pub async fn backfill_unanalyzed(
        &self,
        after_id: i64,
        limit: i64,
    ) -> Result<BackfillResult, ApiError> {
        let queue = self
            .ingestion
            .as_ref()
            .ok_or_else(|| ApiError::Internal("ingestion queue not configured".into()))?;
        let limit = limit.clamp(1, MAX_BACKFILL_LIMIT);
        let ids = self.repo.unanalyzed_post_ids(after_id, limit).await?;

        let mut enqueued = 0i64;
        let mut last_id = after_id;
        for id in ids {
            queue
                .enqueue(id)
                .await
                .map_err(|err| ApiError::Internal(format!("backfill enqueue failed: {err}")))?;
            enqueued += 1;
            last_id = id;
        }
        Ok(BackfillResult { enqueued, last_id })
    }

    /// Read-only ingestion progress over the corpus: how many posts are analysed
    /// vs not, and the analysed counts broken down by model version. Touches no
    /// signal payload and enqueues nothing (ADR 0006).
    pub async fn ingestion_status(&self) -> Result<IngestionStatus, ApiError> {
        // The two reads are independent — run them concurrently.
        let (unanalyzed, rows) = tokio::try_join!(
            self.repo.count_unanalyzed_posts(),
            self.repo.signals_count_by_model_version(),
        )?;
        let analyzed = rows.iter().map(|(_, count)| count).sum();
        let by_model_version = rows.into_iter().collect();
        Ok(IngestionStatus {
            analyzed,
            unanalyzed,
            by_model_version,
        })
    }
}

/// A report of a non-existent post hits the FK — a client error (404), not a fault.
fn map_report_error(err: sqlx::Error) -> ApiError {
    ApiError::on_fk_violation(err, ApiError::NotFound)
}

/// With media pre-validated above, the ONLY foreign key the insert can now trip is
/// the author — a non-existent author is a client error (bad author), not a server
/// fault, so surface it as a 400. (Media never reaches the FK anymore: a missing or
/// not-owned asset is rejected as `unknown_media` before the insert.)
fn map_create_error(err: sqlx::Error) -> ApiError {
    ApiError::on_fk_violation(err, ApiError::Validation("unknown_author"))
}
