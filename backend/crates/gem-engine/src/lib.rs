//! The Gamma weight math — a pure, deterministic function.
//!
//! `compute_payouts(inputs, params, emission)` is IDENTICAL in Phase 1a (off-chain,
//! no value) and Phase 1b (real PT). Only the ledger backing changes, never this.
//! Payout apportionment is integer largest-remainder (Hamilton) so the distributed
//! sum equals the emission EXACTLY — invariant (i) Conservation (Dossier App. B.2).

use domain::{PtAmount, UserId};
use econ_params::EconParams;
use std::collections::BTreeMap;

pub mod graph;
pub use graph::{build_user_inputs, Edge, UserMeta};

/// Per-user inputs for one epoch, already snapshotted from the interaction graph.
#[derive(Debug, Clone)]
pub struct UserInputs {
    pub user: UserId,
    /// Hard bot gate v_i: false ⇒ weight 0, unconditionally.
    pub verified: bool,
    /// Decayed interaction volume Σ ω_type · e^{-λτ}. The `e^{-λτ}` time-decay is
    /// applied to each edge's weight by the caller (settlement) before it reaches
    /// the engine — the engine sums the weights it is given.
    pub interaction_volume: f64,
    /// Node score NS_i (PageRank), strictly positive for connected users.
    pub node_score: f64,
    /// Unique users / unique interaction-types term U_i (diminishing).
    pub unique_factor: f64,
    /// Audience attracted a_i — bot-gated impressions on the user's content.
    pub audience: f64,
    /// Cumulative account-burn B_i in sats (drives the β multiplier).
    pub account_burn_sats: u64,
}

/// Concave burn multiplier β(B) = 1 + κ·ln(1 + B/B0).
///
/// Strictly increasing and strictly concave: wealth cannot buy dominance linearly
/// (Dossier §4.3). Used for account-burn → weight and post-burn → visibility, on
/// separate κ knobs.
pub fn beta_multiplier(burn_sats: u64, kappa: f64, scale_sats: u64) -> f64 {
    let scale = scale_sats.max(1) as f64;
    1.0 + kappa * (1.0 + burn_sats as f64 / scale).ln()
}

/// Linear social weight w_i, built in log-space then exponentiated.
///
/// log w_i = log(volume) + log β(B_i) + log NS_i + log(1+U_i) + γ·log(1+a_i)·NS_i
/// with the hard gate v_i ∈ {0,1}. Log-space keeps correlated connectivity terms
/// additive so a hub/sybil cannot multiply them into an explosion (Dossier §4.3).
pub fn weight(input: &UserInputs, params: &EconParams) -> f64 {
    if !input.verified {
        return 0.0;
    }
    let beta = beta_multiplier(
        input.account_burn_sats,
        params.kappa_account,
        params.burn_scale_sats,
    );
    let log_w = input.interaction_volume.max(f64::MIN_POSITIVE).ln()
        + beta.ln()
        + input.node_score.max(f64::MIN_POSITIVE).ln()
        + (1.0 + input.unique_factor).ln()
        + params.gamma_audience * (1.0 + input.audience).ln() * input.node_score;
    log_w.exp()
}

/// Column-stochastic PageRank via power iteration. `transition[j][i]` is the share
/// of user i's outgoing weighted engagement directed at j (columns sum to 1).
/// Returns normalised scores summing to 1. Banach guarantees a unique fixed point;
/// iteration converges geometrically at the damping rate (Dossier §4.3).
pub fn pagerank(transition: &[Vec<f64>], damping: f64, iters: usize) -> Vec<f64> {
    let n = transition.len();
    if n == 0 {
        return vec![];
    }
    let mut rank = vec![1.0 / n as f64; n];
    let teleport = (1.0 - damping) / n as f64;
    for _ in 0..iters {
        let mut next = vec![teleport; n];
        for (j, next_j) in next.iter_mut().enumerate() {
            for (i, &rank_i) in rank.iter().enumerate() {
                *next_j += damping * transition[j][i] * rank_i;
            }
        }
        rank = next;
    }
    rank
}

/// Distribute `emission` PT across users by normalised weight, using integer
/// largest-remainder apportionment so Σ payouts == emission EXACTLY. Deterministic:
/// ties break by user id, so a retry recomputes bit-identically.
pub fn compute_payouts(
    inputs: &[UserInputs],
    params: &EconParams,
    emission: PtAmount,
) -> BTreeMap<UserId, PtAmount> {
    let weights: Vec<(UserId, f64)> = inputs.iter().map(|i| (i.user, weight(i, params))).collect();
    let total: f64 = weights.iter().map(|(_, w)| *w).sum();

    let mut out: BTreeMap<UserId, PtAmount> = BTreeMap::new();
    if total <= 0.0 || emission.0 == 0 {
        for (u, _) in &weights {
            out.insert(*u, PtAmount(0));
        }
        return out;
    }

    let e = emission.0;
    let mut allocated: u128 = 0;
    // (user, fractional remainder, floor allocation)
    let mut parts: Vec<(UserId, f64, u128)> = Vec::with_capacity(weights.len());
    for (u, w) in &weights {
        let exact = (*w / total) * e as f64;
        let floor = exact.floor() as u128;
        allocated += floor;
        parts.push((*u, exact - floor as f64, floor));
    }

    // Hand out the leftover one unit at a time to the largest remainders.
    let mut leftover = e - allocated;
    parts.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    for p in parts.iter_mut() {
        if leftover == 0 {
            break;
        }
        p.2 += 1;
        leftover -= 1;
    }

    for (u, _, amt) in parts {
        out.insert(u, PtAmount(amt));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(n: u64, verified: bool) -> Vec<UserInputs> {
        (0..n)
            .map(|i| UserInputs {
                user: UserId(i),
                verified,
                interaction_volume: i as f64 + 1.0,
                node_score: 0.1 * (i as f64 + 1.0),
                unique_factor: i as f64,
                audience: 2.0 * i as f64,
                account_burn_sats: 1000 * i,
            })
            .collect()
    }

    #[test]
    fn conservation_holds_exactly() {
        let params = EconParams::default();
        let payouts = compute_payouts(&sample(7, true), &params, PtAmount(5_753_000_000_000));
        let sum: u128 = payouts.values().map(|p| p.0).sum();
        assert_eq!(sum, 5_753_000_000_000);
    }

    #[test]
    fn unverified_users_earn_nothing() {
        let params = EconParams::default();
        let mut inputs = sample(1, false);
        inputs.extend(sample(1, true).into_iter().map(|mut u| {
            u.user = UserId(99);
            u
        }));
        let payouts = compute_payouts(&inputs, &params, PtAmount(1_000_000));
        assert_eq!(payouts[&UserId(0)].0, 0, "bot gate is an absolute veto");
    }

    #[test]
    fn pagerank_sums_to_one() {
        // Two nodes pointing at each other.
        let m = vec![vec![0.0, 1.0], vec![1.0, 0.0]];
        let r = pagerank(&m, 0.85, 100);
        let s: f64 = r.iter().sum();
        assert!((s - 1.0).abs() < 1e-9);
    }

    proptest::proptest! {
        /// Conservation must hold for ANY emission and any reasonable user set.
        #[test]
        fn prop_conservation(emission in 0u128..1_000_000_000_000u128, n in 1u64..50u64) {
            let params = EconParams::default();
            let payouts = compute_payouts(&sample(n, true), &params, PtAmount(emission));
            let sum: u128 = payouts.values().map(|p| p.0).sum();
            proptest::prop_assert_eq!(sum, emission);
        }
    }
}
