//! Core API library (Phase 1a): users, posts, feed.
//!
//! Layering convention used EVERYWHERE: `handler → service → repository`. Each
//! domain lives in its own module folder (see `users/`) with that exact split,
//! so a reviewer who learns one folder can read them all.
//!
//! The app is a library; the binary (`src/main.rs`) is a thin bootstrap around
//! it. That split lets integration tests drive the real router in-process.

pub mod error;
pub mod feed;
pub mod follows;
pub mod interactions;
pub mod posts;
pub mod state;
pub mod users;

mod health;

use axum::Router;

pub use state::AppState;

/// Build the full router with all routes mounted and state injected.
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(health::routes())
        .merge(users::handler::routes())
        .merge(posts::handler::routes())
        .merge(follows::handler::routes())
        .merge(feed::handler::routes())
        .merge(interactions::handler::routes())
        .with_state(state)
}
