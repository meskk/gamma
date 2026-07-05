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

    // --- Interaction edge weights ω_type (see gem-engine matrix M) ---
    /// Per-interaction-type edge weights. These directly scale every user's payout
    /// share, so they are as economic as `time_decay_lambda` and belong here, not
    /// hardcoded in the API. A retune is a `version` bump. (Historical
    /// `interaction_events.weight` rows keep the ω they were stamped with; read them
    /// alongside the epoch's `econ.version` to know which economy produced them.)
    pub interaction_weights: InteractionWeights,

    // --- Referral (MASTERPLAN P-2) ---
    /// Default referrer cut of a referred user's epoch payout (basis points;
    /// 300 = 3%). Snapshotted onto each referral at registration; per-creator
    /// contract overrides live in the `referral_terms` table, not here. The cut
    /// is conserving — taken OUT of the referred user's payout, never minted.
    pub referral_bps_default: u16,
    /// How many epochs (days) a referral earns, counted from the referred
    /// user's registration. Default 183 ≈ 6 months of daily epochs.
    pub referral_duration_epochs: u64,

    // --- Genesis (Phase 1b only) ---
    /// Genesis LP seed target in sats — a depth/cold-start knob, not a solvency lock.
    /// $100k @ $60k/BTC ≈ 1.667 BTC.
    pub genesis_seed_target_sats: u64,
}

/// ω_type edge weights per interaction type (like < comment < share; dwell is a
/// weak signal; follow is structural). Tunable via config, never in code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractionWeights {
    pub like: f64,
    pub comment: f64,
    pub share: f64,
    pub follow: f64,
    pub dwell: f64,
}

impl Default for InteractionWeights {
    fn default() -> Self {
        Self {
            like: 1.0,
            comment: 3.0,
            share: 5.0,
            follow: 2.0,
            dwell: 0.5,
        }
    }
}

impl Default for EconParams {
    /// Proposed defaults from Rebuild Dossier v5 §10.
    fn default() -> Self {
        Self {
            // v2: referral knobs added (owner decision 2026-07-05: 3% / 6 months
            // default, per-creator overrides in the referral_terms table).
            version: 2,
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
            interaction_weights: InteractionWeights::default(),
            referral_bps_default: 300,
            referral_duration_epochs: 183,
            genesis_seed_target_sats: 166_666_667,
        }
    }
}

/// A semantically-invalid parameter set (e.g. take-rates summing past 100%).
#[derive(Debug, PartialEq, Eq)]
pub struct ParamError(pub String);

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid econ-params: {}", self.0)
    }
}

impl std::error::Error for ParamError {}

impl EconParams {
    /// Load a parameter set from a TOML string (e.g. a per-environment config file).
    /// The result is `validate()`d so a malformed config fails at load, not silently
    /// at settlement time (a fee+burn > 100% would credit creators nothing and
    /// corrupt the burn journal).
    pub fn from_toml_str(s: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let params: Self = toml::from_str(s)?;
        params.validate()?;
        Ok(params)
    }

    /// Serialize back to TOML — handy for snapshotting the exact knobs an epoch ran under.
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Reject economically-nonsensical knobs before they can reach the money path.
    /// Bad values here don't error at settlement — they silently mis-distribute or,
    /// worse, break conservation of the burn journal. Fail closed at the boundary.
    pub fn validate(&self) -> Result<(), ParamError> {
        let bps = |name: &str, v: u16| -> Result<(), ParamError> {
            if v > 10_000 {
                return Err(ParamError(format!("{name} = {v} bps exceeds 100%")));
            }
            Ok(())
        };
        bps("company_skim_bps", self.company_skim_bps)?;
        bps("content_fee_bps", self.content_fee_bps)?;
        bps("transfer_burn_bps", self.transfer_burn_bps)?;
        bps("referral_bps_default", self.referral_bps_default)?;
        bps("emission_decay_bps", self.emission_decay_bps)?;
        // A paid unlock splits price into creator + fee + burn; fee+burn > 100%
        // would drive the creator's share negative.
        if self.content_fee_bps as u32 + self.transfer_burn_bps as u32 > 10_000 {
            return Err(ParamError(format!(
                "content_fee_bps ({}) + transfer_burn_bps ({}) exceeds 100%",
                self.content_fee_bps, self.transfer_burn_bps
            )));
        }
        // Damping must be a strict contraction in (0,1) for PageRank to converge to
        // a unique fixed point with a non-negative teleport term.
        if !(0.0..1.0).contains(&self.pagerank_damping) || self.pagerank_damping <= 0.0 {
            return Err(ParamError(format!(
                "pagerank_damping = {} must be in (0, 1)",
                self.pagerank_damping
            )));
        }
        if self.time_decay_lambda < 0.0 || !self.time_decay_lambda.is_finite() {
            return Err(ParamError(format!(
                "time_decay_lambda = {} must be finite and >= 0 (negative makes older interactions count MORE)",
                self.time_decay_lambda
            )));
        }
        for (name, v) in [
            ("kappa_account", self.kappa_account),
            ("kappa_post", self.kappa_post),
            ("gamma_audience", self.gamma_audience),
        ] {
            if v < 0.0 || !v.is_finite() {
                return Err(ParamError(format!("{name} = {v} must be finite and >= 0")));
            }
        }
        let w = &self.interaction_weights;
        for (name, v) in [
            ("like", w.like),
            ("comment", w.comment),
            ("share", w.share),
            ("follow", w.follow),
            ("dwell", w.dwell),
        ] {
            if v < 0.0 || !v.is_finite() {
                return Err(ParamError(format!(
                    "interaction_weights.{name} = {v} must be finite and >= 0"
                )));
            }
        }
        Ok(())
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

    #[test]
    fn defaults_are_valid() {
        assert!(EconParams::default().validate().is_ok());
    }

    #[test]
    fn rejects_fee_plus_burn_over_100pct() {
        let p = EconParams {
            content_fee_bps: 6000,
            transfer_burn_bps: 6000,
            ..Default::default()
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_bad_damping_and_negative_lambda() {
        for p in [
            EconParams {
                pagerank_damping: 1.0,
                ..Default::default()
            },
            EconParams {
                pagerank_damping: 0.0,
                ..Default::default()
            },
            EconParams {
                time_decay_lambda: -0.1,
                ..Default::default()
            },
        ] {
            assert!(p.validate().is_err());
        }
    }

    #[test]
    fn from_toml_str_rejects_invalid() {
        let p = EconParams {
            company_skim_bps: 20_000,
            ..Default::default()
        };
        let s = p.to_toml_string().unwrap();
        assert!(EconParams::from_toml_str(&s).is_err());
    }
}
