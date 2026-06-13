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
