//! Follow edge: `follower_id` follows `followee_id`. Both are user BIGSERIALs;
//! the pair is the primary key, so an edge is unique.

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Follow {
    pub follower_id: i64,
    pub followee_id: i64,
    pub created_at: DateTime<Utc>,
}
