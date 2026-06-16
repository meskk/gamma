//! The `feed` domain — read-only ranking over posts + follows.
//!
//! It has no entity of its own; it reuses `posts::model::Post`, so there is no
//! `model` module here. The repository runs the bounded candidate query (Dossier
//! Appendix A.2); the service applies the Phase-1 cold-start ranker. A per-user
//! ML ranker is a Phase-2 replacement (Dossier §4.2) — same interface.

pub mod handler;
pub mod repository;
pub mod service;

pub use service::FeedService;
