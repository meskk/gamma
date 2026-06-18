//! Versioned economic parameters — "the knobs".
//!
//! Every economic constant lives here, never hardcoded elsewhere. A tokenomics
//! change is a `version` bump of this struct plus a re-audit — NOT a code rewrite.
//! This is the crate that makes Antonio's "must survive tokenomics changes"
//! requirement true. See docs/adr/0003-economic-params-are-config.md.
//!
//! The project targets the v6 economic spine (ADR 0007); v6 KEEPS the 1a weight
//! math + take-rates and only changes the money rail in Phase 1b. So most defaults
//! below carry over unchanged, while a few knobs are v5/Phase-1b artifacts not used
//! by the 1a code yet — `genesis_seed_target_sats` (v5 LP seed; v6 removes it) and
//! the BTC-sats `burn_scale_sats`/`*_burn` framing — kept until the 1b rebuild
//! lands. Nothing is locked; a change is a `version` bump, not a code rewrite.

use domain::PT_ONE;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconParams {
    /// Schema version of this parameter set. Bump on every change.
    pub version: u32,

    // --- Company take-rates (basis points; 200 = 2%) ---
    /// Buy-and-burn skim to the company. Dossier default 2%.
    pub company_skim_bps: u16,
    /// Content-marketplace fee on a paid unlock. Dossier default 2% (range 1–2%).
    pub content_fee_bps: u16,
    /// Fixed burn on a paid unlock / transfer. Dossier 2% — deflation, not a company knob.
    pub transfer_burn_bps: u16,

    // --- Emission schedule (fixed; never coupled to burns or demand) ---
    /// Daily emission in PT base units, year 0. Dossier: 5,753 PT/day → 21M cap.
    /// `u64` (not `u128`) because TOML has no u128; 5,753 PEER fits with huge headroom.
    pub emission_day0_pt: u64,
    /// Annual emission decay (basis points; 1000 = 10%/yr).
    pub emission_decay_bps: u16,

    // --- Weight math (see gem-engine) ---
    /// PageRank damping α. Dossier 0.85.
    pub pagerank_damping: f64,
    /// Interaction time-decay λ per day. Dossier ≈0.099 (~7-day half-life).
    pub time_decay_lambda: f64,
    /// Account-burn → weight sensitivity κ_account. Dossier 0.3 (keep low/0 early).
    pub kappa_account: f64,
    /// Post-burn → visibility sensitivity κ_post. Dossier 0.3.
    pub kappa_post: f64,
    /// Audience-term weight γ. Dossier 1.0.
    pub gamma_audience: f64,
    /// Burn-multiplier scale B0 in sats. Dossier 0.001 BTC = 100_000 sats.
    pub burn_scale_sats: u64,

    // --- Genesis (Phase 1b only) ---
    /// Genesis LP seed target in sats — a depth/cold-start knob, not a solvency lock.
    /// $100k @ $60k/BTC ≈ 1.667 BTC.
    pub genesis_seed_target_sats: u64,
}

impl Default for EconParams {
    /// Proposed defaults from Rebuild Dossier v5 §10.
    fn default() -> Self {
        Self {
            version: 1,
            company_skim_bps: 200,
            content_fee_bps: 200,
            transfer_burn_bps: 200,
            emission_day0_pt: (5_753 * PT_ONE) as u64,
            emission_decay_bps: 1000,
            pagerank_damping: 0.85,
            time_decay_lambda: 0.099,
            kappa_account: 0.3,
            kappa_post: 0.3,
            gamma_audience: 1.0,
            burn_scale_sats: 100_000,
            genesis_seed_target_sats: 166_666_667,
        }
    }
}

impl EconParams {
    /// Load a parameter set from a TOML string (e.g. a per-environment config file).
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Serialize back to TOML — handy for snapshotting the exact knobs an epoch ran under.
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let p = EconParams::default();
        let s = p.to_toml_string().unwrap();
        let back = EconParams::from_toml_str(&s).unwrap();
        assert_eq!(p, back);
    }
}
