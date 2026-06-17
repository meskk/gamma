//! Core API library (Phase 1a): users, posts, feed.
//!
//! Layering convention used EVERYWHERE: `handler → service → repository`. Each
//! domain lives in its own module folder (see `users/`) with that exact split,
//! so a reviewer who learns one folder can read them all.
//!
//! The app is a library; the binary (`src/main.rs`) is a thin bootstrap around
//! it. That split lets integration tests drive the real router in-process.

pub mod auth;
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

use axum::extract::DefaultBodyLimit;
use axum::Router;

pub use state::AppState;

/// Max request body we accept. All bodies are small JSON (media bytes go directly
/// to object storage via presigned URLs, never through the API), so a tight cap
/// bounds memory and rejects oversized payloads early. Tighter than axum's 2 MB
/// default, deliberately.
const MAX_BODY_BYTES: usize = 256 * 1024;

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
        .merge(follows::handler::routes())
        .merge(feed::handler::routes())
        .merge(interactions::handler::routes())
        .merge(gems::handler::routes())
        .merge(media::handler::routes())
        .merge(signals::handler::routes());

    Router::new()
        .merge(health::routes())
        .nest("/v1", v1)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}
