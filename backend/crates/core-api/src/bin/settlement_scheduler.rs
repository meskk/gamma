//! Settlement scheduler: periodically settles the just-closed epoch(s) so the gem
//! economy runs without a manual `POST /epochs/:k/settle`. Run alongside the API:
//! `cargo run --bin settlement_scheduler`.
//!
//! Single-instance by intent. Settlement is idempotent ("mint, then mark"), so a
//! restart or a missed tick is harmless — each tick re-settles a small window of
//! recently-closed epochs, catching up anything skipped during downtime.

use std::time::Duration;

use chrono::Utc;
use core_api::gems::service::SettlementService;
use domain::Epoch;

/// How many recently-closed epochs to (re-)settle each tick — a catch-up window
/// for downtime. Idempotency makes re-settling already-done epochs a cheap no-op.
const LOOKBACK: i64 = 3;
/// Poll cadence. Epochs are daily, so this only needs to be well under a day; a
/// few minutes keeps settlement prompt after an epoch closes without busy-looping.
const TICK: Duration = Duration::from_secs(300);

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
    let pool = db::connect(&database_url, 5).await?;
    let settlement = SettlementService::with_econ(pool, core_api::load_econ_params());

    tracing::info!("settlement scheduler started (lookback {LOOKBACK} epochs)");
    loop {
        let current = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i64;
        match settlement.settle_closed_epochs(current, LOOKBACK).await {
            Ok(summaries) => {
                for s in summaries.iter().filter(|s| !s.already_settled) {
                    tracing::info!(
                        epoch = s.epoch_k,
                        emission = s.emission,
                        users = s.user_count,
                        "settled epoch"
                    );
                }
            }
            Err(err) => tracing::error!(error = ?err, "settlement tick failed; will retry"),
        }
        tokio::time::sleep(TICK).await;
    }
}
