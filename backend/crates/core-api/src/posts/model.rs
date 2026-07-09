//! Post data types: the persisted row (`Post`) and the create request (`NewPost`).
//!
//! `category` and `body` are nullable in the schema, so the stored row uses
//! `Option<String>`. `NewPost` requires a `body` (the API rejects empty ones);
//! `author_id` is the BIGSERIAL of the authoring user (FK into `users`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub category: Option<String>,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Cold-start feed signal (popularity + recency); 0 at creation (Dossier §4.2).
    pub popularity_score: f64,
    /// Optional attached media asset (image/video/audio). Access is gated by the
    /// asset's own unlock_price — see `GET /media/:id`.
    pub media_id: Option<i64>,
    /// `public` or `private` (P-4, migration 0021). A private post is the
    /// creator's paywalled area; it must never surface in a read path unless the
    /// viewer is entitled — enforced in the repository queries (see the
    /// post-visibility invariant doc). A post projected here is one the viewer
    /// may already see, so this field is a display hint (e.g. a lock badge), not
    /// itself the gate.
    pub area: String,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct NewPost {
    /// Set by the server from the authenticated session — never read from the
    /// request body (skip_deserializing), so a client can't post as someone else.
    /// Omitted from the TS contract: the frontend never sends it.
    #[serde(skip_deserializing)]
    #[ts(skip)]
    pub author_id: i64,
    #[serde(default)]
    pub category: Option<String>,
    pub body: String,
    /// Optional id of an already-uploaded media asset to attach. The frontend sets
    /// this after the presigned-upload + finalize flow.
    #[serde(default)]
    pub media_id: Option<i64>,
    /// `public` (default) or `private` (P-4/A4g). A private post is the creator's
    /// paywalled-area content: hidden from every read path unless the viewer is
    /// entitled, never analysed by ingestion. The value is whitelisted in the
    /// service; the DB CHECK (migration 0021) is the fail-closed backstop.
    #[serde(default = "default_area")]
    pub area: String,
}

fn default_area() -> String {
    "public".to_string()
}

/// A user's report of a post (moderation). The reporter is the session user.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct ReportRequest {
    pub reason: String,
}

/// Operator review row: a reported post with how many reports it has.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct ReportedPost {
    pub post_id: i64,
    pub report_count: i64,
    pub hidden: bool,
    /// `public` or `private` — so the operator queue shows whether a reported
    /// post is paywalled (the queue deliberately still lists private posts,
    /// ADR 0011 §5).
    pub area: String,
}
