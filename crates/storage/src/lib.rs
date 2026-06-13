//! S3-compatible object storage client (MinIO locally, any S3 store in prod).
//!
//! Media bytes NEVER pass through the app server: clients upload and download via
//! short-lived presigned URLs issued here. The app only tracks metadata and hands
//! out URLs, so long video/audio scale on the object store + CDN, not our API.

use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage operation failed: {0}")]
    S3(String),
    #[error("presigning failed: {0}")]
    Presign(String),
}

type Result<T> = std::result::Result<T, StorageError>;

/// Connection details for the object store. Sourced from env (see `.env.example`).
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

impl StorageConfig {
    /// Read from `S3_*` env vars, defaulting to the local MinIO from docker-compose.
    pub fn from_env() -> Self {
        Self {
            endpoint: env_or("S3_ENDPOINT", "http://localhost:9000"),
            region: env_or("S3_REGION", "us-east-1"),
            bucket: env_or("S3_BUCKET", "gamma-media"),
            access_key: env_or("S3_ACCESS_KEY", "gamma"),
            secret_key: env_or("S3_SECRET_KEY", "gammasecret"),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[derive(Clone)]
pub struct Storage {
    client: Client,
    bucket: String,
}

impl Storage {
    pub fn new(cfg: StorageConfig) -> Self {
        let creds = Credentials::new(cfg.access_key, cfg.secret_key, None, None, "static");
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(cfg.region))
            .endpoint_url(cfg.endpoint)
            .credentials_provider(creds)
            // MinIO and most self-hosted S3 stores need path-style addressing.
            .force_path_style(true)
            .build();
        Self {
            client: Client::from_conf(conf),
            bucket: cfg.bucket,
        }
    }

    /// Create the bucket if it doesn't already exist. Safe to call on startup.
    pub async fn ensure_bucket(&self) -> Result<()> {
        match self
            .client
            .create_bucket()
            .bucket(&self.bucket)
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                let svc = err.into_service_error();
                if svc.is_bucket_already_owned_by_you() || svc.is_bucket_already_exists() {
                    Ok(())
                } else {
                    Err(StorageError::S3(svc.to_string()))
                }
            }
        }
    }

    /// Presigned PUT URL — the client uploads bytes directly to the store. The
    /// `content_type` is part of the signature, so the upload must send it too.
    pub async fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        ttl: Duration,
    ) -> Result<String> {
        let cfg =
            PresigningConfig::expires_in(ttl).map_err(|e| StorageError::Presign(e.to_string()))?;
        let req = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .presigned(cfg)
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;
        Ok(req.uri().to_string())
    }

    /// Presigned GET URL — the client downloads/streams directly from the store.
    pub async fn presign_get(&self, key: &str, ttl: Duration) -> Result<String> {
        let cfg =
            PresigningConfig::expires_in(ttl).map_err(|e| StorageError::Presign(e.to_string()))?;
        let req = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;
        Ok(req.uri().to_string())
    }

    /// Download an object's bytes. Used by the transcoding worker to read a
    /// source upload; not on any user request path.
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;
        let data = out
            .body
            .collect()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;
        Ok(data.into_bytes().to_vec())
    }

    /// Upload bytes server-side (the worker writes HLS segments/manifest here).
    /// User uploads use presigned PUT instead — this never runs on a request path.
    pub async fn put_object(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(bytes))
            .send()
            .await
            .map_err(|e| StorageError::S3(e.to_string()))?;
        Ok(())
    }

    /// Object size in bytes if it exists, `None` if it doesn't — used to confirm
    /// an upload actually landed before marking a media asset ready.
    pub async fn head(&self, key: &str) -> Result<Option<i64>> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(Some(out.content_length().unwrap_or(0))),
            Err(err) => {
                let svc = err.into_service_error();
                if svc.is_not_found() {
                    Ok(None)
                } else {
                    Err(StorageError::S3(svc.to_string()))
                }
            }
        }
    }
}
