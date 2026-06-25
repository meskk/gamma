//! Comment types: the persisted row (`Comment`) and the create request (`NewComment`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub author_id: i64,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// Create request. `post_id` comes from the path and `author_id` from the session,
/// so the body carries only the text.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct NewComment {
    pub body: String,
}
