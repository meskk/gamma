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

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (see .env.example)"))?;

    let pool = db::connect(&database_url, 10).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("database connected and migrations applied");

    let state = AppState::new(pool);
    state.storage.ensure_bucket().await?;
    tracing::info!("object storage bucket ready");

    // Per-IP rate limit, applied at the server edge only (not inside `app()`, so
    // the in-process test router is unaffected). Steady ~10 req/s with bursts to
    // 50 — blunts login/argon2 brute-force and general request flooding. Keyed on
    // the peer IP, which is why we serve with connect-info below.
    let governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(10)
            .burst_size(50)
            .finish()
            .expect("valid rate-limit config"),
    );
    let service = app(state).layer(GovernorLayer { config: governor });

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
