//! Content-signal shapes. Since ADR 0009 the `signals` document has a VERSIONED
//! contract: `schema_version` says which contract it follows (0 = legacy
//! free-form, 1 = the typed v1 core validated in the service), `model_version`
//! says who produced it. The v1 core itself stays `serde_json::Value` here —
//! validation lives in the service, so the wire shape never constrains what
//! future schema versions may look like.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A stored content-signal row: the pipeline's analysis of one post.
#[derive(Debug, Clone, Serialize)]
pub struct ContentSignal {
    pub post_id: i64,
    pub model_version: String,
    pub schema_version: i16,
    pub signals: Value,
    pub updated_at: DateTime<Utc>,
}

/// Write-back request body from the ingestion service.
#[derive(Debug, Clone, Deserialize)]
pub struct SignalWriteback {
    /// Which model/version produced these signals (so a re-analysis can supersede).
    pub model_version: String,
    /// Which signal CONTRACT `signals` follows (ADR 0009). Absent = 0 = legacy
    /// free-form; 1 = typed core, validated on write; anything newer than the
    /// API knows is rejected (fail closed).
    #[serde(default)]
    pub schema_version: i16,
    /// The signal document. Contract per `schema_version`; under 0 the API does
    /// not interpret it at all (pre-ADR behavior).
    pub signals: Value,
    /// Optional post embedding (ADR 0009 §3) — stored in `post_embeddings`,
    /// never inside the signals JSONB and never returned by the read path.
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
}
