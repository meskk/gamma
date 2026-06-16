//! The `users` domain — the template every other domain (posts, feed, …) copies.
//!
//! - `model`      — the persisted row and request/response shapes
//! - `repository` — the only place that knows users SQL
//! - `service`    — business rules (validation, normalisation)
//! - `handler`    — HTTP translation + route table
//!
//! Data flows handler → service → repository; nothing skips a layer.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::UserService;
