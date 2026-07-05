//! The `feed` domain — read-only ranking over posts + follows.
//!
//! Items reuse `posts::model::Post`; `model` only wraps them in the paginated
//! `FeedPage` (B1). The repository runs the bounded candidate query (Dossier
//! Appendix A.2); the service applies the Phase-1 cold-start ranker. A per-user
//! ML ranker is a Phase-2 replacement (Dossier §4.2) — same interface.
//!
//! DEFERRED BOUNDARY (ADR 0006): this domain MUST NOT consume `content_signals`
//! yet. Concretely, until a FUTURE ADR (gated on the dossier §4.2 taxonomy) defines
//! how signals feed ranking, do not: read `content_signals` in `service::score()`,
//! join it into `repository::candidates()`, add signal-derived fields to `Post`, or
//! add ts-rs derives that would freeze a signal shape into the frontend contract.
//! The AI pipeline only WRITES signals (and they can be read back operator-only);
//! ranking stays on `popularity_score`/recency/category until the shape is settled.

pub mod cursor;
pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::FeedService;
