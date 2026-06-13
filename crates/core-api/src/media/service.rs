//! Media business logic: issue upload tickets, finalize uploads, and build
//! playback views — coordinating the Postgres metadata with the object store.

use std::time::Duration;

use storage::Storage;
use uuid::Uuid;

use crate::error::ApiError;
use crate::media::model::{MediaAsset, MediaAssetView, NewUpload, UploadTicket};
use crate::media::repository::MediaRepository;
use crate::media::transcode;
use db::PgPool;

/// How long an upload ticket is valid.
const UPLOAD_TTL: Duration = Duration::from_secs(15 * 60);
/// How long a playback URL is valid.
const PLAYBACK_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub struct MediaService {
    repo: MediaRepository,
    storage: Storage,
}

impl MediaService {
    pub fn new(pool: PgPool, storage: Storage) -> Self {
        Self {
            repo: MediaRepository::new(pool),
            storage,
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

        let asset = self
            .repo
            .create(
                req.owner_id,
                req.kind.as_str(),
                &object_key,
                &req.content_type,
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
    pub async fn finalize(&self, asset_id: i64) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;

        let size = self
            .storage
            .head(&asset.object_key)
            .await?
            .ok_or(ApiError::Validation("not_uploaded"))?;

        let ready = self.repo.mark_ready(asset.id, size).await?;
        self.view(ready).await
    }

    /// Fetch an asset with a playback URL (present only once ready).
    pub async fn get(&self, asset_id: i64) -> Result<MediaAssetView, ApiError> {
        let asset = self.repo.get(asset_id).await?.ok_or(ApiError::NotFound)?;
        self.view(asset).await
    }

    /// Transcode a ready video/audio asset to HLS: download the source, run
    /// ffmpeg, upload the manifest + segments under `<object_key>/hls/`, and
    /// record the manifest key. Synchronous for now; M3b moves it to a worker.
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
        self.view(updated).await
    }

    async fn view(&self, asset: MediaAsset) -> Result<MediaAssetView, ApiError> {
        let playback_url = if asset.status == "ready" {
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
        })
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
