//! Follow edge: `follower_id` follows `followee_id`. Both are user BIGSERIALs;
//! the pair is the primary key, so an edge is unique.

use chrono::{DateTime, Utc};
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct Follow {
    pub follower_id: i64,
    pub followee_id: i64,
    pub created_at: DateTime<Utc>,
}
