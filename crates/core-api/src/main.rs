//! Binary bootstrap: load env, open the pool, apply migrations, serve.
//!
//! All logic lives in the library (`lib.rs`) so integration tests can drive the
//! same router in-process. This file only wires the process to the outside world.

use core_api::{app, AppState};

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

    let bind = std::env::var("CORE_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("core-api listening on http://{bind}");
    axum::serve(listener, app(state)).await?;
    Ok(())
}
