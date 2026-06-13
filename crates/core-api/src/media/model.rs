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
    /// Set by the server from the authenticated session (skip_deserializing).
    #[serde(skip_deserializing)]
    pub owner_id: i64,
    pub kind: MediaKind,
    /// MIME type, e.g. "video/mp4". Its top-level type must match `kind`.
    pub content_type: String,
    /// PT price to unlock this asset. 0 (default) means free / open.
    #[serde(default)]
    pub unlock_price: i64,
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
    pub hls_manifest_key: Option<String>,
    pub transcode_status: String,
    pub unlock_price: i64,
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
    /// True once an HLS rendition has been produced.
    pub hls_ready: bool,
    /// PT price to unlock; 0 means free.
    pub unlock_price: i64,
}

/// Result of a paid unlock — the split is reported for transparency.
#[derive(Debug, Clone, Serialize)]
pub struct UnlockSummary {
    pub asset_id: i64,
    pub viewer_id: i64,
    pub price: i64,
    pub creator_received: i64,
    pub company_fee: i64,
    pub burned: i64,
    /// True if the viewer had already unlocked it (no charge applied).
    pub already_unlocked: bool,
}
