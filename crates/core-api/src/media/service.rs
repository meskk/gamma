//! Media business logic: issue upload tickets, finalize uploads, and build
//! playback views — coordinating the Postgres metadata with the object store.

use std::time::Duration;

use chrono::Utc;
use domain::Epoch;
use storage::Storage;
use uuid::Uuid;

use crate::error::ApiError;
use crate::media::model::{MediaAsset, MediaAssetView, NewUpload, UnlockSummary, UploadTicket};
use crate::media::repository::{MediaRepository, UnlockError};
use crate::media::transcode;
use crate::queue::TranscodeQueue;
use db::PgPool;

/// How long an upload ticket is valid.
const UPLOAD_TTL: Duration = Duration::from_secs(15 * 60);
/// How long a direct download / playback URL is valid.
const PLAYBACK_TTL: Duration = Duration::from_secs(60 * 60);
/// HLS segment URLs must stay valid for a whole viewing session, so use a longer
/// TTL. (Prod uses CDN signed cookies, which avoid per-URL expiry entirely.)
const HLS_SEGMENT_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Sentinel account that receives the company fee. Not a real user (BIGSERIAL
/// starts at 1), and gem_balances has no FK, so id 0 is a safe company bucket.
const COMPANY_ACCOUNT_ID: i64 = 0;

#[derive(Clone)]
pub struct MediaService {
    repo: MediaRepository,
    storage: Storage,
    queue: TranscodeQueue,
}

impl MediaService {
    pub fn new(pool: PgPool, storage: Storage, queue: TranscodeQueue) -> Self {
        Self {
            repo: MediaRepository::new(pool),
            storage,
            queue,
        }
    }

    /// Create a pending asset and return a presigned PUT URL to upload to.
    pub async fn create_upload(&self, req: NewUpload) -> Result<UploadTicket, ApiError> {
        // The content-type's top-level type must match the declared kind.
        let top_level = req.content_type.split('/').next().unwrap_or("");
        if top_level != req.kind.as_str() {
            return Err(ApiError::Validation("content_type_mismatch"));
        }

        // Opaque, non-enumerable storage key.
        let object_key = format!("media/{}/{}", req.kind.as_str(), Uuid::new_v4());

        if req.unlock_price < 0 {
            return Err(ApiError::Validation("negative_price"));
        }

        let asset = self
            .repo
            .create(
                req.owner_id,
                req.kind.as_str(),
                &object_key,
                &req.content_type,
                req.unlock_price,
            )
            .await
            .map_err(map_create_error)?;

        let upload_url = self
            .storage
            .presign_put(&asset.object_key, &asset.content_type, UPLOAD_TTL)
            .await?;

        Ok(UploadTicket {
            asset_id: asset.id,
            object_key: asset.object_key,
            upload_url,
            expires_in_secs: UPLOAD_TTL.as_secs(),
        })
    }

    /// Confirm the upload landed (HEAD the object) and mark the asset ready.
    /// Owner-only: only the uploader may finalize their own asset.
    pub async fn finalize(
        &self,
        asset_id: i64,
        requester_id: i64,
    ) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;
        if asset.owner_id != requester_id {
            return Err(ApiError::Forbidden);
        }

        let size = self
            .storage
            .head(&asset.object_key)
            .await?
            .ok_or(ApiError::Validation("not_uploaded"))?;

        let ready = self.repo.mark_ready(asset.id, size).await?;

        // Queue transcoding for playable media. A failure to enqueue is logged,
        // not fatal — the asset is still usable and can be transcoded via the
        // manual endpoint or a re-finalize.
        if ready.kind == "video" || ready.kind == "audio" {
            if let Err(err) = self.queue.enqueue(ready.id).await {
                tracing::warn!(asset_id = ready.id, error = %err, "failed to enqueue transcode job");
            }
        }

        // The owner just finalized their own asset, so they are entitled.
        self.view(ready, true).await
    }

    /// Mark an asset's transcode as failed (used by the worker on ffmpeg errors).
    pub async fn mark_failed(&self, asset_id: i64) -> Result<(), ApiError> {
        self.repo.set_transcode_status(asset_id, "failed").await?;
        Ok(())
    }

    /// Fetch an asset. The raw `playback_url` (a presigned URL to the full-quality
    /// original) is exposed ONLY to a viewer entitled to it — the owner, free
    /// content, or someone who has unlocked it. Everyone else still sees metadata
    /// (price, status) so a client can prompt an unlock, but never the raw file.
    pub async fn get(&self, asset_id: i64, viewer_id: i64) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;
        let entitled = self.is_entitled(&asset, viewer_id).await?;
        self.view(asset, entitled).await
    }

    /// Owner-gated entry for the manual HTTP transcode trigger. The async worker
    /// calls `transcode` directly (it is trusted, off-band); any HTTP caller must
    /// own the asset.
    pub async fn transcode_owned(
        &self,
        asset_id: i64,
        requester_id: i64,
    ) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;
        if asset.owner_id != requester_id {
            return Err(ApiError::Forbidden);
        }
        self.transcode(asset_id).await
    }

    /// Transcode a ready video/audio asset to HLS: download the source, run
    /// ffmpeg, upload the manifest + segments under `<object_key>/hls/`, and
    /// record the manifest key. Called by the worker (trusted) and, via
    /// `transcode_owned`, by the owner's manual HTTP trigger.
    pub async fn transcode(&self, asset_id: i64) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;
        if asset.status != "ready" {
            return Err(ApiError::Validation("not_ready"));
        }
        let is_video = asset.kind == "video";
        if !is_video && asset.kind != "audio" {
            return Err(ApiError::Validation("not_transcodable"));
        }

        let source = self.storage.get_object(&asset.object_key).await?;
        let output = transcode::to_hls(&source, is_video)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let prefix = format!("{}/hls", asset.object_key);
        for file in &output.files {
            let key = format!("{prefix}/{}", file.name);
            self.storage
                .put_object(&key, file.bytes.clone(), file.content_type)
                .await?;
        }

        let manifest_key = format!("{prefix}/{}", output.manifest_name);
        let updated = self.repo.set_hls(asset.id, &manifest_key).await?;
        // Transcode is reached only by the owner (HTTP) or the trusted worker
        // (result discarded); either way the raw URL is safe to include here.
        self.view(updated, true).await
    }

    /// Pay to unlock an asset: split the price into creator share, company fee,
    /// and burn (rates from econ-params), then apply it atomically. Owner-owned
    /// and free assets need no unlock.
    pub async fn unlock(&self, asset_id: i64, viewer_id: i64) -> Result<UnlockSummary, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;

        if asset.status != "ready" {
            return Err(ApiError::Validation("not_ready"));
        }
        if asset.unlock_price <= 0 {
            return Err(ApiError::Validation("free_content"));
        }
        if viewer_id == asset.owner_id {
            return Err(ApiError::Validation("owner_already_entitled"));
        }

        // Split per the versioned knobs (content fee + fixed burn); the creator
        // gets the remainder so creator + fee + burn == price exactly.
        let params = econ_params::EconParams::default();
        let price = asset.unlock_price;
        let company_fee = price * params.content_fee_bps as i64 / 10_000;
        let burned = price * params.transfer_burn_bps as i64 / 10_000;
        let creator_received = price - company_fee - burned;
        // Stamp the journal entries with the current epoch (unlocks aren't
        // epoch-scoped, but the journal records when value moved).
        let epoch_k = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i64;

        let outcome = self
            .repo
            .unlock(
                viewer_id,
                asset.owner_id,
                COMPANY_ACCOUNT_ID,
                asset_id,
                price,
                creator_received,
                company_fee,
                burned,
                epoch_k,
            )
            .await
            .map_err(map_unlock_error)?;

        Ok(UnlockSummary {
            asset_id,
            viewer_id,
            price,
            creator_received,
            company_fee,
            burned,
            already_unlocked: outcome.already_unlocked,
        })
    }

    /// Access-controlled HLS manifest. The viewer must own the asset, the asset
    /// must be free, or the viewer must have unlocked it — otherwise 402. The
    /// returned manifest has each segment rewritten to a short-lived presigned
    /// URL, so the heavy segment bytes still come straight from the object store.
    pub async fn manifest(&self, asset_id: i64, viewer_id: i64) -> Result<String, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;

        let manifest_key = match &asset.hls_manifest_key {
            Some(key) if asset.transcode_status == "done" => key,
            _ => return Err(ApiError::Validation("not_transcoded")),
        };

        if !self.is_entitled(&asset, viewer_id).await? {
            return Err(ApiError::PaymentRequired);
        }

        let raw = self.storage.get_object(manifest_key).await?;
        let manifest = String::from_utf8_lossy(&raw);
        let prefix = format!("{}/hls", asset.object_key);

        // Rewrite each segment line (non-comment, non-empty) to a presigned URL.
        let mut out = String::with_capacity(manifest.len());
        for line in manifest.lines() {
            if line.is_empty() || line.starts_with('#') {
                out.push_str(line);
            } else {
                let url = self
                    .storage
                    .presign_get(&format!("{prefix}/{line}"), HLS_SEGMENT_TTL)
                    .await?;
                out.push_str(&url);
            }
            out.push('\n');
        }
        Ok(out)
    }

    /// Whether `viewer` may access the asset's content: the owner, free content,
    /// or a recorded unlock. Single source of truth for both the raw playback URL
    /// (in `view`) and the HLS manifest gate (in `manifest`).
    async fn is_entitled(&self, asset: &MediaAsset, viewer_id: i64) -> Result<bool, ApiError> {
        Ok(viewer_id == asset.owner_id
            || asset.unlock_price <= 0
            || self.repo.is_unlocked(viewer_id, asset.id).await?)
    }

    /// Build the public view. `entitled` gates the raw `playback_url`: a presigned
    /// URL to the full-quality original is included only for an entitled viewer.
    async fn view(&self, asset: MediaAsset, entitled: bool) -> Result<MediaAssetView, ApiError> {
        let playback_url = if asset.status == "ready" && entitled {
            Some(
                self.storage
                    .presign_get(&asset.object_key, PLAYBACK_TTL)
                    .await?,
            )
        } else {
            None
        };
        Ok(MediaAssetView {
            id: asset.id,
            owner_id: asset.owner_id,
            kind: asset.kind,
            content_type: asset.content_type,
            status: asset.status,
            size_bytes: asset.size_bytes,
            playback_url,
            hls_ready: asset.transcode_status == "done",
            unlock_price: asset.unlock_price,
        })
    }
}

/// Map the unlock transaction error: insufficient gems is a client problem.
fn map_unlock_error(err: UnlockError) -> ApiError {
    match err {
        UnlockError::InsufficientFunds => ApiError::Validation("insufficient_gems"),
        UnlockError::Db(e) => ApiError::Database(e),
    }
}

/// An upload for a non-existent owner hits the FK constraint — a client error.
fn map_create_error(err: sqlx::Error) -> ApiError {
    if let Some(db_err) = err.as_database_error() {
        if matches!(db_err.kind(), sqlx::error::ErrorKind::ForeignKeyViolation) {
            return ApiError::Validation("unknown_owner");
        }
    }
    ApiError::Database(err)
}
