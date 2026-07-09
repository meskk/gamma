//! The private area (P-4, ADR 0011) — the non-custodial creator marketplace.
//!
//! Each profile has a fourth tab whose access model the CREATOR chooses:
//! free, one-time price, subscription, or per-item. Purchases run over an
//! external payment provider (Stripe stage 1, wallet stage 2) — customer
//! money never touches a platform account, and fiat NEVER touches the
//! conserved PT journal (`purchases` is an audit mirror, not a ledger).
//! Access materializes as an ENTITLEMENT (row with optional expiry), never a
//! payment lookup at read time. Data layer landed in A2; the visibility
//! invariant (posts AND their media) lands in A4; provider seam in A5/A6.
//! Everything stays behind config flags until legal sign-off (ADR 0011 §6).

pub mod model;
pub mod repository;
