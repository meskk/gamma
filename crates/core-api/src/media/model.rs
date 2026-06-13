//! Media types: kind enum, the persisted asset row, and request/response shapes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
            MediaKind::Audio => "audio",
        }
    }
}

/// Request to begin an upload.
#[derive(Debug, Clone, Deserialize)]
pub struct NewUpload {
    pub owner_id: i64,
    pub kind: MediaKind,
    /// MIME type, e.g. "video/mp4". Its top-level type must match `kind`.
    pub content_type: String,
}

/// The persisted asset row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MediaAsset {
    pub id: i64,
    pub owner_id: i64,
    pub kind: String,
    pub object_key: String,
    pub content_type: String,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub created_at: DateTime<Utc>,
}

/// What the client needs to upload directly to the object store.
#[derive(Debug, Clone, Serialize)]
pub struct UploadTicket {
    pub asset_id: i64,
    pub object_key: String,
    /// Presigned PUT URL — upload the bytes here with the declared content-type.
    pub upload_url: String,
    pub expires_in_secs: u64,
}

/// Public view of an asset. `object_key` is internal and omitted; `playback_url`
/// is a presigned GET URL, present only once the asset is ready.
#[derive(Debug, Clone, Serialize)]
pub struct MediaAssetView {
    pub id: i64,
    pub owner_id: i64,
    pub kind: String,
    pub content_type: String,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub playback_url: Option<String>,
}
