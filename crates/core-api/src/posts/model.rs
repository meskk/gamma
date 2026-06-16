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
}
