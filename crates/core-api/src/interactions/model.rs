//! Interaction types and event shapes.
//!
//! The interaction *type* is a typed enum at the API boundary but stored as a
//! compact `smallint` code in the DB. Each type carries an edge weight ω_type
//! (like < comment < share, per Dossier §4.3) that feeds the column-normalised
//! interaction matrix `M`. The weights are tunable and may later move to the
//! calibration constants.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InteractionType {
    Like,
    Comment,
    Share,
    Follow,
    Dwell,
}

impl InteractionType {
    /// Stable smallint code stored in the DB. Never renumber existing codes.
    pub fn code(self) -> i16 {
        match self {
            InteractionType::Like => 0,
            InteractionType::Comment => 1,
            InteractionType::Share => 2,
            InteractionType::Follow => 3,
            InteractionType::Dwell => 4,
        }
    }

    pub fn from_code(code: i16) -> Option<Self> {
        match code {
            0 => Some(InteractionType::Like),
            1 => Some(InteractionType::Comment),
            2 => Some(InteractionType::Share),
            3 => Some(InteractionType::Follow),
            4 => Some(InteractionType::Dwell),
            _ => None,
        }
    }

    /// ω_type — the edge weight this interaction contributes to the graph.
    /// like < comment < share; dwell is a weak signal; follow is structural.
    pub fn weight(self) -> f64 {
        match self {
            InteractionType::Dwell => 0.5,
            InteractionType::Like => 1.0,
            InteractionType::Follow => 2.0,
            InteractionType::Comment => 3.0,
            InteractionType::Share => 5.0,
        }
    }
}

/// Request to record an interaction. `target_id` (the other user) and `post_id`
/// are optional — a like targets a post, a follow targets a user, etc.
#[derive(Debug, Clone, Deserialize)]
pub struct NewInteraction {
    pub actor_id: i64,
    pub r#type: InteractionType,
    #[serde(default)]
    pub target_id: Option<i64>,
    #[serde(default)]
    pub post_id: Option<i64>,
}

/// A persisted interaction event (append-only). `type_code` is the stored code;
/// callers use `InteractionView` for a typed representation.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InteractionEvent {
    pub id: i64,
    pub actor_id: i64,
    pub target_id: Option<i64>,
    pub post_id: Option<i64>,
    pub type_code: i16,
    pub weight: f64,
    pub created_at: DateTime<Utc>,
    pub epoch_k: i32,
}

/// API representation: typed `type`, no raw code, no internal timestamp noise.
#[derive(Debug, Clone, Serialize)]
pub struct InteractionView {
    pub id: i64,
    pub actor_id: i64,
    pub target_id: Option<i64>,
    pub post_id: Option<i64>,
    pub r#type: InteractionType,
    pub weight: f64,
    pub epoch_k: i32,
}

impl InteractionView {
    /// Build the view from a stored event. The code is always one we wrote, so
    /// `from_code` cannot fail in practice; an unknown code falls back to `Like`
    /// rather than panicking on a corrupt row.
    pub fn from_event(event: &InteractionEvent) -> Self {
        Self {
            id: event.id,
            actor_id: event.actor_id,
            target_id: event.target_id,
            post_id: event.post_id,
            r#type: InteractionType::from_code(event.type_code).unwrap_or(InteractionType::Like),
            weight: event.weight,
            epoch_k: event.epoch_k,
        }
    }
}
