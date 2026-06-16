//! The `gems` domain — the off-chain epoch settlement worker (Phase 1a).
//!
//! It closes the loop: read an epoch's interaction edges, resolve verified
//! users, build the gem-engine inputs, and settle (mint gems by weight) into the
//! Postgres ledger — idempotently, guarded by `epoch_settlements`.
//!
//! This is where the platform's captured data finally becomes the gem economy.
//! In Phase 1b the same `settle_epoch` runs against the Solana-backed ledger.

pub mod handler;
pub mod model;
pub mod repository;
pub mod service;

pub use service::SettlementService;
