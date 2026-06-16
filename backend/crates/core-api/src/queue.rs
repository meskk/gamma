//! Redis-backed transcode job queue.
//!
//! A finalized video/audio asset is enqueued (LPUSH) and the transcode worker
//! consumes it (RPOP) out of band, so ffmpeg never blocks a request. A Redis LIST
//! is enough at our scale; a stream with acks is a later durability upgrade.

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

    /// Push an asset id onto the queue.
    pub async fn enqueue(&self, asset_id: i64) -> Result<(), QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = conn.lpush(&self.key, asset_id).await?;
        Ok(())
    }

    /// Pop the next asset id, or `None` if the queue is empty (non-blocking).
    pub async fn dequeue(&self) -> Result<Option<i64>, QueueError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let id: Option<i64> = conn.rpop(&self.key, None).await?;
        Ok(id)
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
