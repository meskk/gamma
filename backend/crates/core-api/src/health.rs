//! Liveness and readiness probes.
//!
//! `/health` is liveness — the process is up; it never touches the database.
//! `/ready` is readiness — the database is reachable; returns 503 otherwise, so a
//! load balancer stops routing traffic here until it recovers.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "core-api",
    })
}

async fn ready(State(state): State<AppState>) -> Result<Json<Health>, StatusCode> {
    match db::ping(&state.pool).await {
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}
