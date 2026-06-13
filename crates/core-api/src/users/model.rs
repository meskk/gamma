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

#[derive(Debug, Clone, Deserialize)]
pub struct NewUser {
    #[serde(default)]
    pub declared_categories: Vec<String>,
    #[serde(default)]
    pub bot_gate_v: bool,
}
