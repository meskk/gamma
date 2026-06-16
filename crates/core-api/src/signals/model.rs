//! Content-signal shapes. `signals` is an opaque JSON document by design — the
//! ingestion pipeline owns its structure and can evolve it without a Rust change.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A stored content-signal row: the pipeline's analysis of one post.
#[derive(Debug, Clone, Serialize)]
pub struct ContentSignal {
    pub post_id: i64,
    pub model_version: String,
    pub signals: Value,
    pub updated_at: DateTime<Utc>,
}

/// Write-back request body from the ingestion service.
#[derive(Debug, Clone, Deserialize)]
pub struct SignalWriteback {
    /// Which model/version produced these signals (so a re-analysis can supersede).
    pub model_version: String,
    /// Opaque pipeline output (topic, quality, …); the API does not interpret it.
    pub signals: Value,
}
