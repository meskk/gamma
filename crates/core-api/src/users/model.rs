//! User data types: the persisted row (`User`) and the create request (`NewUser`).
//!
//! `id` is the Postgres BIGSERIAL (`i64`). The economic engine's `domain::UserId`
//! is `u64`; that conversion happens at the engine boundary (ids are positive
//! serials, so it is lossless), not here — the CRUD layer stays DB-native.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub declared_categories: Vec<String>,
    /// Bot gate v_i: manual early, heuristic later, KYC in Phase 2 (Dossier §4.4).
    pub bot_gate_v: bool,
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
#[derive(Debug, Clone, Deserialize)]
pub struct VerificationRequest {
    pub verified: bool,
}
