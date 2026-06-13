//! Turn a set of interaction edges into per-user `UserInputs` for one epoch.
//!
//! This is the bridge between captured `interaction_events` and the pure weight
//! math: it builds the column-normalised matrix `M`, runs PageRank for the node
//! score, and derives the volume / uniqueness / audience terms from the graph.
//!
//! INTERPRETATION NOTE (Phase 1a, tunable — flagged for review). The Dossier's
//! "interaction volume" term is ambiguous, so we take it as the total weight of
//! edges INCIDENT to a user (incoming + outgoing). That stops a pure creator
//! (lots of incoming engagement, little outgoing) from being zeroed by `log(0)`.
//! `audience` (a_i) is proxied by incoming weighted degree until real impression
//! capture exists; `unique_factor` is the count of distinct counterparties. These
//! are economic-model choices and may change — they live behind this one function.

use std::collections::{BTreeMap, BTreeSet};

use domain::UserId;
use econ_params::EconParams;

use crate::{pagerank, UserInputs};

/// A directed, weighted interaction: `actor` engaged `target` with `weight`
/// (the ω_type of the interaction).
#[derive(Debug, Clone)]
pub struct Edge {
    pub actor: UserId,
    pub target: UserId,
    pub weight: f64,
}

/// Per-user data that the edges alone don't carry.
#[derive(Debug, Clone)]
pub struct UserMeta {
    pub user: UserId,
    pub verified: bool,
    pub account_burn_sats: u64,
}

/// PageRank power-iteration count. 100 is ample for graphs at our scale to reach
/// the fixed point at damping 0.85.
const PAGERANK_ITERS: usize = 100;

/// Assemble `UserInputs` for every user that appears in `edges` or `meta`.
pub fn build_user_inputs(
    edges: &[Edge],
    meta: &[UserMeta],
    params: &EconParams,
) -> Vec<UserInputs> {
    // 1. Node set: anyone in an edge or carrying metadata. Sorted for determinism.
    let mut nodes: BTreeSet<UserId> = BTreeSet::new();
    for e in edges {
        nodes.insert(e.actor);
        nodes.insert(e.target);
    }
    for m in meta {
        nodes.insert(m.user);
    }
    let index: Vec<UserId> = nodes.into_iter().collect();
    let n = index.len();
    if n == 0 {
        return vec![];
    }
    let pos: BTreeMap<UserId, usize> = index.iter().enumerate().map(|(i, u)| (*u, i)).collect();
    let meta_map: BTreeMap<UserId, &UserMeta> = meta.iter().map(|m| (m.user, m)).collect();

    // 2. Accumulate aggregates and the raw (unnormalised) transition weights.
    //    transition[j][i] = weight of i's outgoing engagement directed at j.
    let mut out_weight = vec![0.0f64; n];
    let mut in_weight = vec![0.0f64; n];
    let mut counterparties: Vec<BTreeSet<UserId>> = vec![BTreeSet::new(); n];
    let mut transition = vec![vec![0.0f64; n]; n];

    for e in edges {
        let i = pos[&e.actor];
        let j = pos[&e.target];
        out_weight[i] += e.weight;
        in_weight[j] += e.weight;
        transition[j][i] += e.weight;
        counterparties[i].insert(e.target);
        counterparties[j].insert(e.actor);
    }

    // 3. Column-normalise. A dangling actor (no outgoing weight) links uniformly
    //    so PageRank rank is conserved rather than leaking away.
    let uniform = 1.0 / n as f64;
    for i in 0..n {
        if out_weight[i] > 0.0 {
            for row in transition.iter_mut() {
                row[i] /= out_weight[i];
            }
        } else {
            for row in transition.iter_mut() {
                row[i] = uniform;
            }
        }
    }

    // 4. Node score via PageRank over the column-stochastic matrix.
    let ranks = pagerank(&transition, params.pagerank_damping, PAGERANK_ITERS);

    // 5. Assemble inputs per node.
    index
        .iter()
        .enumerate()
        .map(|(i, user)| {
            let m = meta_map.get(user);
            UserInputs {
                user: *user,
                verified: m.map(|m| m.verified).unwrap_or(false),
                interaction_volume: out_weight[i] + in_weight[i],
                node_score: ranks[i],
                unique_factor: counterparties[i].len() as f64,
                audience: in_weight[i],
                account_burn_sats: m.map(|m| m.account_burn_sats).unwrap_or(0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(a: u64, b: u64, w: f64) -> Edge {
        Edge {
            actor: UserId(a),
            target: UserId(b),
            weight: w,
        }
    }

    #[test]
    fn empty_graph_yields_no_inputs() {
        assert!(build_user_inputs(&[], &[], &EconParams::default()).is_empty());
    }

    #[test]
    fn aggregates_volume_audience_and_counterparties() {
        // A -> B (1.0), B -> A (3.0), C -> A (5.0)
        let edges = [edge(1, 2, 1.0), edge(2, 1, 3.0), edge(3, 1, 5.0)];
        let meta = [
            UserMeta {
                user: UserId(1),
                verified: true,
                account_burn_sats: 0,
            },
            UserMeta {
                user: UserId(2),
                verified: true,
                account_burn_sats: 0,
            },
            UserMeta {
                user: UserId(3),
                verified: true,
                account_burn_sats: 0,
            },
        ];
        let inputs = build_user_inputs(&edges, &meta, &EconParams::default());
        assert_eq!(inputs.len(), 3);

        let a = inputs.iter().find(|i| i.user == UserId(1)).unwrap();
        // A: out 1.0 (to B), in 3.0+5.0 = 8.0 → volume 9.0, audience 8.0,
        // counterparties {B, C} = 2.
        assert_eq!(a.interaction_volume, 9.0);
        assert_eq!(a.audience, 8.0);
        assert_eq!(a.unique_factor, 2.0);

        // PageRank ranks are a probability distribution.
        let total: f64 = inputs.iter().map(|i| i.node_score).sum();
        assert!((total - 1.0).abs() < 1e-6);
        // A receives the most incoming weight, so it should rank highest.
        assert!(
            a.node_score
                > inputs
                    .iter()
                    .find(|i| i.user == UserId(2))
                    .unwrap()
                    .node_score
        );
    }

    #[test]
    fn missing_meta_defaults_to_unverified() {
        // B has no metadata entry → treated as unverified (earns nothing).
        let edges = [edge(1, 2, 1.0)];
        let meta = [UserMeta {
            user: UserId(1),
            verified: true,
            account_burn_sats: 0,
        }];
        let inputs = build_user_inputs(&edges, &meta, &EconParams::default());
        let b = inputs.iter().find(|i| i.user == UserId(2)).unwrap();
        assert!(!b.verified);
    }
}
