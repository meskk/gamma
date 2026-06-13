//! The `interactions` domain — append-only capture of the social interaction
//! graph (Dossier Appendix B.1).
//!
//! WHY THIS MATTERS NOW: epoch boundaries cannot be reconstructed retroactively,
//! so events must be stamped with their epoch the moment they happen. Capturing
//! from day one is cheap; backfilling later is impossible. These events feed both
//! the feed ranker and the Gamma node score (PageRank input `M`).

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::InteractionService;
