//! Core API (Phase 1a): users, posts, feed.
//!
//! Layering convention used EVERYWHERE in this crate: `handler → service →
//! repository`. A reviewer who learns one route knows how to read them all.
//! Postgres (sqlx) gets wired in the next step; this is the runnable skeleton.

use axum::{routing::get, Json, Router};
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

/// handler — translates HTTP to/from the service layer. No business logic here.
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "core-api",
    })
}

fn app() -> Router {
    Router::new().route("/health", get(health))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind = std::env::var("CORE_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("core-api listening on http://{bind}");
    axum::serve(listener, app()).await?;
    Ok(())
}
