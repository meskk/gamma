//! Content signals — the AI ingestion pipeline's write-back target.
//!
//! New posts are offered to the (external, later) ingestion service via the
//! `gamma:ingestion` queue (see `queue::IngestionQueue`, enqueued in
//! `PostService::create`). The service analyses the content and writes its result
//! back here via the operator-only `PUT /posts/:id/signals` endpoint, so all DB
//! writes stay behind the API. The feed will consume these signals once their
//! shape is settled. Contract: docs/adr/0006.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::SignalService;
