//! Transcode worker: consumes the Redis queue and runs ffmpeg out of band, so
//! the API never blocks on transcoding. Run alongside the API: `cargo run --bin
//! transcode_worker`.

use std::time::Duration;

use core_api::media::MediaService;
use core_api::queue::TranscodeQueue;
use core_api::worker::process_one;
use storage::{Storage, StorageConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (see .env.example)"))?;
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

    let pool = db::connect(&database_url, 5).await?;
    let storage = Storage::new(StorageConfig::from_env());
    storage.ensure_bucket().await?;
    let queue = TranscodeQueue::new(&redis_url)?;
    let media = MediaService::new(pool, storage, queue.clone());

    tracing::info!("transcode worker started");
    loop {
        match process_one(&media, &queue).await {
            // Handled a job — immediately check for the next one.
            Some(_) => {}
            // Queue empty — back off briefly before polling again.
            None => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    }
}
