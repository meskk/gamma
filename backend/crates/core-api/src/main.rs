//! Binary bootstrap: load env, open the pool, apply migrations, serve.
//!
//! All logic lives in the library (`lib.rs`) so integration tests can drive the
//! same router in-process. This file only wires the process to the outside world.

use std::net::SocketAddr;
use std::sync::Arc;

use core_api::{app, AppState};
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

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

    let pool = db::connect(&database_url, 10).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("database connected and migrations applied");

    let state = AppState::new(pool);
    state.storage.ensure_bucket().await?;
    tracing::info!("object storage bucket ready");

    // Per-IP rate limit, applied at the server edge only (not inside `app()`, so the
    // in-process test router is unaffected). Blunts login/argon2 brute-force and
    // request flooding; keyed on the peer IP (why we serve with connect-info below).
    //
    // Env-configurable: the typed SPA fires several requests per page (doubled again
    // by React StrictMode in dev), so a tight global limit throttles normal use —
    // set GAMMA_RATE_LIMIT_DISABLED=true locally. NOTE: this is a coarse GLOBAL limit;
    // per-route limits (tight on /auth, loose on reads) are the proper follow-up, at
    // which point the default here can be raised for production.
    let mut service = app(state);
    if std::env::var("GAMMA_RATE_LIMIT_DISABLED").as_deref() == Ok("true") {
        tracing::warn!("per-IP rate limit DISABLED (GAMMA_RATE_LIMIT_DISABLED=true)");
    } else {
        let per_second: u64 = std::env::var("GAMMA_RATE_LIMIT_PER_SECOND")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let burst: u32 = std::env::var("GAMMA_RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);
        let governor = Arc::new(
            GovernorConfigBuilder::default()
                .per_second(per_second)
                .burst_size(burst)
                .finish()
                .expect("valid rate-limit config"),
        );
        service = service.layer(GovernorLayer { config: governor });
        tracing::info!(per_second, burst, "per-IP rate limit enabled");
    }

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
