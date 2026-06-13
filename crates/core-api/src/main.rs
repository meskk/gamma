//! Core API (Phase 1a): users, posts, feed.
//!
//! Layering convention used EVERYWHERE in this crate: `handler → service →
//! repository`. A reviewer who learns one route knows how to read them all.
//!
//! Startup: load `.env`, open the Postgres pool, apply migrations, then serve.
//! Two probes: `/health` is liveness (process up, no DB); `/ready` is readiness
//! (the DB is reachable) — the distinction matters for load balancers and deploys.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{routing::get, Json, Router};
use db::PgPool;
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

/// Liveness — the process is up. Never touches the database.
async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "core-api",
    })
}

/// Readiness — the process can reach Postgres. Returns 503 if the DB is down,
/// so a load balancer stops routing traffic here until it recovers.
async fn ready(State(pool): State<PgPool>) -> Result<Json<Health>, StatusCode> {
    match db::ping(&pool).await {
        Ok(()) => Ok(Json(Health {
            status: "ready",
            service: "core-api",
        })),
        Err(err) => {
            tracing::error!(%err, "readiness check failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

fn app(pool: PgPool) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .with_state(pool)
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

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (see .env.example)"))?;

    let pool = db::connect(&database_url, 10).await?;
    db::run_migrations(&pool).await?;
    tracing::info!("database connected and migrations applied");

    let bind = std::env::var("CORE_API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("core-api listening on http://{bind}");
    axum::serve(listener, app(pool)).await?;
    Ok(())
}
