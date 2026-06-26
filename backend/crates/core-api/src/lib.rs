//! Core API library (Phase 1a): users, posts, feed.
//!
//! Layering convention used EVERYWHERE: `handler → service → repository`. Each
//! domain lives in its own module folder (see `users/`) with that exact split,
//! so a reviewer who learns one folder can read them all.
//!
//! The app is a library; the binary (`src/main.rs`) is a thin bootstrap around
//! it. That split lets integration tests drive the real router in-process.

pub mod auth;
pub mod comments;
pub mod error;
pub mod feed;
pub mod follows;
pub mod gems;
pub mod interactions;
pub mod media;
pub mod posts;
pub mod queue;
pub mod signals;
pub mod state;
pub mod users;
pub mod worker;

mod health;
mod observability;

pub use observability::install_prometheus;

use axum::extract::DefaultBodyLimit;
use axum::http::{header, HeaderValue, Method, Request};
use axum::routing::get;
use axum::Router;
use econ_params::EconParams;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

pub use state::AppState;

/// Load the economic parameters from the TOML file at `$GAMMA_ECON_PARAMS`, or the
/// built-in defaults if it is unset. Loaded ONCE at startup and threaded into the
/// services that need it, so a deployment can ship a versioned parameter set
/// without a code change (ADR 0003) — this is what makes "the knobs are config,
/// not hardcoded constants" true at runtime rather than aspirational.
pub fn load_econ_params() -> EconParams {
    match std::env::var("GAMMA_ECON_PARAMS") {
        Ok(path) => {
            let toml = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("GAMMA_ECON_PARAMS={path}: cannot read: {e}"));
            let params = EconParams::from_toml_str(&toml)
                .unwrap_or_else(|e| panic!("GAMMA_ECON_PARAMS={path}: invalid: {e}"));
            tracing::info!(
                path,
                version = params.version,
                "loaded econ params from file"
            );
            params
        }
        Err(_) => {
            let params = EconParams::default();
            tracing::info!(version = params.version, "using default econ params");
            params
        }
    }
}

/// One tracing span per HTTP request, carrying the method, path, and the
/// `x-request-id` (set by `SetRequestIdLayer` just outside this) so every log line
/// for a request — and the `x-request-id` echoed on the response — correlate.
fn request_span<B>(req: &Request<B>) -> tracing::Span {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    tracing::info_span!(
        "http",
        method = %req.method(),
        path = %req.uri().path(),
        request_id = %request_id,
    )
}

/// Max request body we accept. All bodies are small JSON (media bytes go directly
/// to object storage via presigned URLs, never through the API), so a tight cap
/// bounds memory and rejects oversized payloads early. Tighter than axum's 2 MB
/// default, deliberately.
const MAX_BODY_BYTES: usize = 256 * 1024;

/// CORS for the browser frontend (a separate origin from the API). The allowed
/// origin is env-driven (`GAMMA_CORS_ORIGIN`, default the local Next.js dev server)
/// so prod can point it at the real frontend without a code change. We allow the
/// bearer `Authorization` + `Content-Type` headers and the methods the API uses;
/// the `CorsLayer` answers preflight (OPTIONS) before routing.
fn cors_layer() -> CorsLayer {
    let origin =
        std::env::var("GAMMA_CORS_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".to_string());
    let origin: HeaderValue = origin
        .parse()
        .unwrap_or_else(|_| panic!("GAMMA_CORS_ORIGIN is not a valid origin: {origin}"));
    CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

/// Build the full router with all routes mounted and state injected.
///
/// The API surface is versioned under `/v1` so the frontend and the (Phase-1b)
/// advertiser API can rely on stable paths — a breaking change ships as `/v2`
/// rather than a coordinated deploy. `/health` and `/ready` stay UNVERSIONED:
/// they are operational probes (load balancers / orchestrators) that belong at a
/// fixed path.
pub fn app(state: AppState) -> Router {
    let v1 = Router::new()
        .merge(auth::handler::routes())
        .merge(users::handler::routes())
        .merge(posts::handler::routes())
        .merge(comments::handler::routes())
        .merge(follows::handler::routes())
        .merge(feed::handler::routes())
        .merge(interactions::handler::routes())
        .merge(gems::handler::routes())
        .merge(media::handler::routes())
        .merge(signals::handler::routes());

    // Layers wrap outermost-last. Order on a request: CORS (answers preflight) →
    // assign x-request-id → open the trace span (reads that id) → propagate the id
    // onto the response → body-limit → metrics → routes. So every request is logged
    // (method/path/status/latency, at INFO) under a span tagged with its id, counted
    // for `/metrics`, and the response carries the id. `/metrics` itself stays
    // UNVERSIONED (a scrape target, like `/health`).
    Router::new()
        .merge(health::routes())
        .route("/metrics", get(observability::metrics_handler))
        .nest("/v1", v1)
        .layer(axum::middleware::from_fn(observability::track_metrics))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_span)
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(cors_layer())
        .with_state(state)
}
