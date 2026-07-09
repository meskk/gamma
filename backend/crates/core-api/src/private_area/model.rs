//! Private-area shapes. Money is integer cents throughout (no floats on
//! money); the access-model vocabulary mirrors the DB CHECK constraint —
//! parse fails closed on anything the schema would reject anyway.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How a creator's private area is accessed — the CREATOR's choice
/// (owner decision 2026-07-08). Mirrors the `private_areas.access_model`
/// CHECK constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessModel {
    Free,
    OneTime,
    Subscription,
    PerPost,
}

impl AccessModel {
    pub fn as_str(self) -> &'static str {
        match self {
            AccessModel::Free => "free",
            AccessModel::OneTime => "one_time",
            AccessModel::Subscription => "subscription",
            AccessModel::PerPost => "per_post",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "free" => Some(AccessModel::Free),
            "one_time" => Some(AccessModel::OneTime),
            "subscription" => Some(AccessModel::Subscription),
            "per_post" => Some(AccessModel::PerPost),
            _ => None,
        }
    }
}

/// One creator's private-area configuration (one row per creator).
#[derive(Debug, Clone)]
pub struct PrivateArea {
    pub creator_id: i64,
    pub access_model: String,
    pub price_cents: i64,
    pub currency: String,
    pub description: String,
    pub updated_at: DateTime<Utc>,
}

/// Request to configure the caller's OWN private area (A3). The creator id
/// comes from the authenticated session, never from the body or path.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct PrivateAreaConfigRequest {
    /// One of `free`, `one_time`, `subscription`, `per_post` (the CREATOR's
    /// choice — owner decision 2026-07-08).
    pub access_model: String,
    /// Integer EUR cents for AREA access: required (> 0) for
    /// one_time/subscription, must be 0 for free/per_post (per-post prices
    /// attach to posts when their stage lands).
    #[serde(default)]
    pub price_cents: i64,
    #[serde(default)]
    pub description: String,
}

/// A creator's private-area terms. This is the public OFFER a potential
/// buyer decides against — nothing in it is secret.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../../bindings/")]
pub struct PrivateAreaView {
    pub creator_id: i64,
    pub access_model: String,
    pub price_cents: i64,
    pub currency: String,
    pub description: String,
}

impl From<PrivateArea> for PrivateAreaView {
    fn from(area: PrivateArea) -> Self {
        Self {
            creator_id: area.creator_id,
            access_model: area.access_model,
            price_cents: area.price_cents,
            currency: area.currency,
            description: area.description,
        }
    }
}

/// Why a viewer holds access — mirrors the `area_entitlements.source` CHECK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementSource {
    Purchase,
    Subscription,
    Operator,
}

impl EntitlementSource {
    pub fn as_str(self) -> &'static str {
        match self {
            EntitlementSource::Purchase => "purchase",
            EntitlementSource::Subscription => "subscription",
            EntitlementSource::Operator => "operator",
        }
    }
}

/// A row in the non-custodial purchases audit mirror. NOT conserved, not a
/// ledger — the provider (Stripe) is the source of truth, this is our
/// nachvollzug for audits and the future P-5 purchase history.
#[derive(Debug, Clone)]
pub struct NewPurchase<'a> {
    pub provider: &'a str,
    /// Provider-side unique reference (e.g. checkout session id) — the
    /// idempotency anchor: replayed webhooks insert nothing twice.
    pub provider_ref: &'a str,
    pub viewer_id: i64,
    pub creator_id: i64,
    pub kind: &'a str,
    pub amount_cents: i64,
    pub currency: &'a str,
    pub fee_cents: i64,
    pub status: &'a str,
}
