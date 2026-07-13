//! Binary bootstrap: load env, open the pool, apply migrations, serve.
//!
//! All logic lives in the library (`lib.rs`) so integration tests can drive the
//! same router in-process. This file only wires the process to the outside world.

use std::net::SocketAddr;

use core_api::rate_limit::{self, AuthRateLimit};
use core_api::util::env_parsed;
use core_api::{app_with_limits, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env if present; real environments set these vars directly.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Install the Prometheus recorder so `/metrics` has data to render. Done in the
    // binary (not in `app()`) so the in-process test router never tries to install a
    // second global recorder.
    core_api::install_prometheus();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (see .env.example)"))?;

    let pool_size: u32 = env_parsed("GAMMA_DB_POOL_SIZE", 10);
    let pool = db::connect(&database_url, pool_size).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("database connected and migrations applied");

    let state = AppState::new(pool);
    state.storage.ensure_bucket().await?;
    tracing::info!("object storage bucket ready");

    // Housekeeping: periodic sweep of expired sessions and stale login-throttle
    // rows. Lives in the API binary — sessions are OWNED by the API (parking this
    // in the settlement scheduler would leak sessions on any deployment that runs
    // the API alone) — and both sweeps are idempotent DELETEs, so several API
    // replicas running them concurrently is harmless. The first tick fires
    // immediately (sweep on boot), then every GAMMA_HOUSEKEEPING_INTERVAL_SECS.
    let auth = state.auth.clone();
    let housekeeping_secs: u64 = env_parsed("GAMMA_HOUSEKEEPING_INTERVAL_SECS", 3600);
    tokio::spawn(async move {
        let mut tick =
            tokio::time::interval(std::time::Duration::from_secs(housekeeping_secs.max(1)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match auth.purge_expired_sessions().await {
                Ok(n) if n > 0 => tracing::info!(removed = n, "purged expired sessions"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = ?e, "session purge failed"),
            }
            match auth.sweep_stale_login_throttle().await {
                Ok(n) if n > 0 => tracing::info!(removed = n, "swept stale login-throttle rows"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = ?e, "login-throttle sweep failed"),
            }
            match auth.sweep_expired_email_codes().await {
                Ok(n) if n > 0 => tracing::info!(removed = n, "swept expired email codes"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = ?e, "email-code sweep failed"),
            }
        }
    });

    // Two per-IP rate-limit layers (both in crate::rate_limit, both off under
    // GAMMA_RATE_LIMIT_DISABLED, both sharing the GAMMA_TRUST_PROXY key choice):
    // a TIGHT bucket on /v1/auth/* (wired inside the router, brute-force pacing)
    // and a LOOSE edge backstop over the whole service (volumetric abuse). The
    // per-account login throttle (auth::throttle) is the third, independent layer.
    let service = rate_limit::edge_from_env(app_with_limits(state, AuthRateLimit::from_env()));

    let bind = std::env::var("CORE_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("core-api listening on http://{bind}");
    axum::serve(
        listener,
        service.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
