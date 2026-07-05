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

/// Column-stochastic PageRank via SPARSE power iteration.
///
/// `out[i]` lists `(target j, share)` for user i's outgoing weighted engagement,
/// the shares in a column summing to 1. A user with no outgoing engagement is
/// "dangling": its column is empty and its rank mass is redistributed uniformly
/// (× damping) each iteration, identical to the dense form where a dangling column
/// was all `1/n`, so rank is conserved rather than leaking away. Returns normalised
/// scores summing to 1; Banach guarantees a unique fixed point, converging
/// geometrically at the damping rate (Dossier §4.3).
///
/// Cost is O(iters · (edges + n)). The dense n×n matrix this replaces was
/// O(iters · n²) and, materialised, made settlement OOM past ~10⁴ users — the
/// interaction graph is sparse (a handful of edges per user per day), so the dense
/// representation, not the math, was the scaling wall.
pub fn pagerank(out: &[Vec<(usize, f64)>], damping: f64, iters: usize) -> Vec<f64> {
    let n = out.len();
    if n == 0 {
        return vec![];
    }
    let inv_n = 1.0 / n as f64;
    let teleport = (1.0 - damping) / n as f64;
    let mut rank = vec![inv_n; n];
    for _ in 0..iters {
        // Rank sitting on dangling nodes (no outgoing edges) is spread uniformly
        // across all nodes this step; every node also gets the teleport term. This
        // base makes each iteration conserve total mass at exactly 1.
        let dangling: f64 = out
            .iter()
            .zip(&rank)
            .filter(|(edges, _)| edges.is_empty())
            .map(|(_, r)| *r)
            .sum();
        let base = teleport + damping * dangling * inv_n;
        let mut next = vec![base; n];
        for (i, edges) in out.iter().enumerate() {
            if edges.is_empty() {
                continue;
            }
            let contrib = damping * rank[i];
            for (j, share) in edges {
                next[*j] += contrib * share;
            }
        }
        rank = next;
    }
    rank
}

/// Bits of integer precision the f64 weight scores are quantised to before
/// apportionment. A user whose score is below `2^-30` of the top score rounds to a
/// zero share — it would round to a zero payout anyway.
const SHARE_BITS: u32 = 30;

/// Distribute `emission` PT across users by normalised weight, using **integer**
/// largest-remainder (Hamilton) apportionment so Σ payouts == emission EXACTLY for
/// ANY emission. Deterministic: ties break by user id, so a retry recomputes
/// bit-identically.
///
/// The float weights are the only floats involved (scoring); the apportionment
/// itself runs entirely in `u128` — the project rule is "no floats on conserved
/// quantities". This is why conservation holds even for emissions past 2^53, where
/// the previous `(w/total) * (e as f64)` form lost precision in `e` and could leave
/// a remainder larger than the user count (breaking the sum).
pub fn compute_payouts(
    inputs: &[UserInputs],
    params: &EconParams,
    emission: PtAmount,
) -> BTreeMap<UserId, PtAmount> {
    let weights: Vec<(UserId, f64)> = inputs.iter().map(|i| (i.user, weight(i, params))).collect();
    let e = emission.0;

    let mut out: BTreeMap<UserId, PtAmount> = BTreeMap::new();
    // `fold` with `f64::max` starts at 0.0 and ignores NaN, so `max_w >= 0.0`
    // always; `max_w <= 0.0` therefore means "no positive weight" (pay nothing).
    let max_w = weights.iter().map(|(_, w)| *w).fold(0.0f64, f64::max);
    if e == 0 || max_w <= 0.0 {
        for (u, _) in &weights {
            out.insert(*u, PtAmount(0));
        }
        return out;
    }

    // Quantise each score into an integer share relative to the top score, so the
    // rest of the math is exact. `share ≤ 2^SHARE_BITS`, and `e` is bounded by the
    // u64 emission schedule, so `share * e` stays well inside u128.
    let scale = (1u64 << SHARE_BITS) as f64 / max_w;
    let shares: Vec<(UserId, u128)> = weights
        .iter()
        .map(|(u, w)| (*u, (w.max(0.0) * scale) as u128))
        .collect();
    let total: u128 = shares.iter().map(|(_, s)| *s).sum();
    if total == 0 {
        for (u, _) in &shares {
            out.insert(*u, PtAmount(0));
        }
        return out;
    }

    let mut allocated: u128 = 0;
    // (user, integer remainder, floor allocation)
    let mut parts: Vec<(UserId, u128, u128)> = Vec::with_capacity(shares.len());
    for (u, s) in &shares {
        let prod = s * e;
        let floor = prod / total;
        let rem = prod % total;
        allocated += floor;
        parts.push((*u, rem, floor));
    }

    // By construction `total * leftover == Σ rem`, and each `rem < total`, so
    // `leftover < n` — exactly `leftover` users get one extra unit, largest
    // remainder first (ties by user id). The sum is then exactly `e`.
    let mut leftover = e - allocated;
    parts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
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
        // Two nodes pointing at each other (sparse columns: 0→1, 1→0).
        let out = vec![vec![(1usize, 1.0)], vec![(0usize, 1.0)]];
        let r = pagerank(&out, 0.85, 100);
        let s: f64 = r.iter().sum();
        assert!((s - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pagerank_conserves_mass_with_dangling_nodes() {
        // Node 0 → 1; node 1 is dangling (no outgoing). Mass must still sum to 1
        // (the dangling rank is redistributed, not lost).
        let out = vec![vec![(1usize, 1.0)], vec![]];
        let r = pagerank(&out, 0.85, 100);
        let s: f64 = r.iter().sum();
        assert!((s - 1.0).abs() < 1e-9, "dangling mass must be conserved");
    }

    #[test]
    fn conservation_holds_for_huge_emission_past_f64_exactness() {
        // Regression: a single dominant user + an emission just above 2^53 used to
        // make `floor(exact) > e`, underflowing the leftover to ~2^128 and breaking
        // conservation (or panicking under overflow-checks). Must conserve exactly.
        let params = EconParams::default();
        let mut inputs = sample(1, true);
        inputs.extend(sample(4, true).into_iter().enumerate().map(|(k, mut u)| {
            u.user = UserId(100 + k as u64);
            u
        }));
        let e: u128 = (1u128 << 53) + 3;
        let payouts = compute_payouts(&inputs, &params, PtAmount(e));
        let sum: u128 = payouts.values().map(|p| p.0).sum();
        assert_eq!(sum, e, "conservation must hold past f64 exactness");
    }

    proptest::proptest! {
        /// Conservation must hold for ANY emission (including values well past 2^53,
        /// which is where f64 rounding of `e` used to break the apportionment) and
        /// any reasonable user set.
        #[test]
        fn prop_conservation(emission in 0u128..u64::MAX as u128, n in 1u64..50u64) {
            let params = EconParams::default();
            let payouts = compute_payouts(&sample(n, true), &params, PtAmount(emission));
            let sum: u128 = payouts.values().map(|p| p.0).sum();
            proptest::prop_assert_eq!(sum, emission);
        }
    }
}
