//! Redis-backed transcode job queue.
//!
//! A finalized video/audio asset is enqueued (LPUSH) and the transcode worker
//! consumes it out of band, so ffmpeg never blocks a request. The worker consumes
//! with a RELIABLE pattern — `RPOPLPUSH` the id onto a processing list, and only
//! `LREM` it off once the job reaches a terminal state (transcoded or marked
//! failed). A crash mid-transcode leaves the id on the processing list, and
//! `recover_stranded()` (run at worker startup) re-queues it, so a job is never
//! lost (at-least-once). Transcoding overwrites its own HLS output, so a re-run is
//! harmless. A single transcode worker is assumed, so one shared processing list
//! suffices.

use redis::AsyncCommands;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
}

const DEFAULT_KEY: &str = "gamma:transcode";

#[derive(Clone)]
pub struct TranscodeQueue {
    client: redis::Client,
    key: String,
}

impl TranscodeQueue {
    /// Construct from a Redis URL. `redis::Client::open` only parses the URL (no
    /// connection yet), so this is cheap and synchronous.
    pub fn new(redis_url: &str) -> Result<Self, QueueError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
            key: DEFAULT_KEY.to_string(),
        })
    }

    /// Construct with an explicit queue key — used by tests for isolation.
    pub fn with_key(redis_url: &str, key: impl Into<String>) -> Result<Self, QueueError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
            key: key.into(),
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// The processing list that holds reserved-but-not-yet-acked jobs.
    fn processing_key(&self) -> String {
        format!("{}:processing", self.key)
    }

    /// Push an asset id onto the queue.
    pub async fn enqueue(&self, asset_id: i64) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = conn.lpush(&self.key, asset_id).await?;
        Ok(())
    }

    /// Pop the next asset id, or `None` if the queue is empty (non-blocking).
    /// Non-reliable (no processing list) — used by tests to assert queue contents.
    /// The worker uses `reserve`/`ack` instead.
    pub async fn dequeue(&self) -> Result<Option<i64>, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let id: Option<i64> = conn.rpop(&self.key, None).await?;
        Ok(id)
    }

    /// Reliably reserve the next job: atomically move its id from the queue onto the
    /// processing list (`RPOPLPUSH`) and return it, or `None` if the queue is empty.
    /// The id stays on the processing list until `ack`ed, so a crash before ack does
    /// not lose it.
    pub async fn reserve(&self) -> Result<Option<i64>, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let id: Option<i64> = conn.rpoplpush(&self.key, self.processing_key()).await?;
        Ok(id)
    }

    /// Acknowledge a reserved job as done (terminal state reached): remove it from
    /// the processing list. Idempotent — a missing entry removes zero.
    pub async fn ack(&self, asset_id: i64) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = conn.lrem(self.processing_key(), 1, asset_id).await?;
        Ok(())
    }

    /// Re-queue every job stranded on the processing list by a prior crash (run once
    /// at worker startup). Returns how many were recovered.
    pub async fn recover_stranded(&self) -> Result<u64, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let mut moved = 0u64;
        loop {
            let id: Option<i64> = conn.rpoplpush(self.processing_key(), &self.key).await?;
            if id.is_none() {
                break;
            }
            moved += 1;
        }
        Ok(moved)
    }
}

const INGESTION_KEY: &str = "gamma:ingestion";

/// Queue offering newly-created content (post ids) to the AI ingestion service —
/// the entry point of the (later, Python/Mac-Studio) pipeline. Same simple Redis
/// LIST mechanism as `TranscodeQueue`; the consumer lives outside this repo and
/// writes its results back via the signals write-back endpoint (see ADR 0006).
#[derive(Clone)]
pub struct IngestionQueue {
    client: redis::Client,
    key: String,
}

impl IngestionQueue {
    pub fn new(redis_url: &str) -> Result<Self, QueueError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
            key: INGESTION_KEY.to_string(),
        })
    }

    /// Construct with an explicit queue key — used by tests for isolation.
    pub fn with_key(redis_url: &str, key: impl Into<String>) -> Result<Self, QueueError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
            key: key.into(),
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Offer a post id to the ingestion pipeline.
    pub async fn enqueue(&self, post_id: i64) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = conn.lpush(&self.key, post_id).await?;
        Ok(())
    }

    /// Pop the next post id, or `None` if empty (non-blocking).
    pub async fn dequeue(&self) -> Result<Option<i64>, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let id: Option<i64> = conn.rpop(&self.key, None).await?;
        Ok(id)
    }
}
