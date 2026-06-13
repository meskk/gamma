//! Post data types: the persisted row (`Post`) and the create request (`NewPost`).
//!
//! `category` and `body` are nullable in the schema, so the stored row uses
//! `Option<String>`. `NewPost` requires a `body` (the API rejects empty ones);
//! `author_id` is the BIGSERIAL of the authoring user (FK into `users`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Post {
    pub id: i64,
    pub author_id: i64,
    pub category: Option<String>,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Cold-start feed signal (popularity + recency); 0 at creation (Dossier §4.2).
    pub popularity_score: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewPost {
    /// Set by the server from the authenticated session — never read from the
    /// request body (skip_deserializing), so a client can't post as someone else.
    #[serde(skip_deserializing)]
    pub author_id: i64,
    #[serde(default)]
    pub category: Option<String>,
    pub body: String,
}
