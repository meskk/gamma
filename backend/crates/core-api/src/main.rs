//! Binary bootstrap: load env, open the pool, apply migrations, serve.
//!
//! All logic lives in the library (`lib.rs`) so integration tests can drive the
//! same router in-process. This file only wires the process to the outside world.

use std::net::SocketAddr;

use core_api::rate_limit::{self, AuthRateLimit};
use core_api::{app_with_limits, AppState};

/// Parse an env var as `T`, or fall back to `default` if it is unset. A PRESENT but
/// unparseable value is a misconfiguration and panics at startup — better than
/// silently falling back to a default the operator didn't intend.
fn env_parsed<T: std::str::FromStr>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("{name} is set but not a valid value: {v:?}")),
        Err(_) => default,
    }
}

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
