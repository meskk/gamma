//! User data types: the persisted row (`User`) and the create request (`NewUser`).
//!
//! `id` is the Postgres BIGSERIAL (`i64`). The economic engine's `domain::UserId`
//! is `u64`; that conversion happens at the engine boundary (ids are positive
//! serials, so it is lossless), not here — the CRUD layer stays DB-native.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, sqlx::FromRow, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct User {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub declared_categories: Vec<String>,
    /// Bot gate v_i: manual early, heuristic later, KYC in Phase 2 (Dossier §4.4).
    pub bot_gate_v: bool,
    /// Public profile stat: DISTINCT (liker, post) pairs with an active
    /// (non-retracted) like on this user's PUBLIC, non-hidden posts (ADR 0012) —
    /// a fan liking three posts counts three, but never twice for the same post
    /// (distinct pairs, not journal rows; see `Post::like_count`). Deliberately
    /// viewer-independent and blind to private-area engagement, so the public
    /// number never leaks paywalled activity. Comment-likes and direct
    /// user-likes are not counted — the stat mirrors what the profile grid
    /// shows: the posts.
    pub likes_received: i64,
    /// Public profile stat: how many users follow THIS user, live from the
    /// `follows` table (the API previously only exposed the *following* list).
    /// Viewer-independent, like `likes_received`.
    pub followers_count: i64,
}

/// Server-side create shape. NOT a client request body: there is no public
/// route that deserializes this. `bot_gate_v` is set by trusted server code
/// (tests, future operator tooling) — a client can never assert its own gate.
#[derive(Debug, Clone, Deserialize)]
pub struct NewUser {
    #[serde(default)]
    pub declared_categories: Vec<String>,
    #[serde(default)]
    pub bot_gate_v: bool,
}

/// Operator-only request to set a user's bot-gate (verified) flag. The gate is
/// the hard veto that decides who earns gems, so it is mutable ONLY through the
/// operator endpoint — never self-asserted at registration.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct VerificationRequest {
    pub verified: bool,
}

/// Operator-only request to set a creator's referral contract (P-2): the cut in
/// basis points and how long referrals recruited FROM NOW ON earn. Terms are
/// frozen onto each referral at registration, so changing a contract never
/// silently rewrites existing referrals.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct ReferralTermsRequest {
    pub bps: i32,
    pub duration_epochs: i64,
    #[serde(default)]
    #[ts(optional)]
    pub note: Option<String>,
}

/// The stored referral contract, echoed back on write.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct ReferralTerms {
    pub referrer_id: i64,
    pub bps: i32,
    pub duration_epochs: i64,
    pub note: Option<String>,
}
