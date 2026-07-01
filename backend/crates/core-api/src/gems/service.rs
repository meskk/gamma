//! The epoch settlement orchestration: edges + verified flags → gem-engine inputs
//! → settle into the Postgres ledger, guarded for idempotency.

use std::collections::BTreeSet;

use chrono::Utc;
use db::PgPool;
use domain::{Epoch, PtAmount, UserId};
use econ_params::EconParams;
use gem_engine::{build_user_inputs, compute_payouts, Edge, UserMeta};
use ledger::{LedgerBackend, PgLedger};
use settlement::emission_for;

use crate::error::ApiError;
use crate::gems::model::{GemBalance, SettlementSummary};
use crate::gems::repository::GemRepository;
use crate::interactions::repository::InteractionRepository;
use crate::users::repository::UserRepository;

#[derive(Clone)]
pub struct SettlementService {
    pool: PgPool,
    econ: EconParams,
}

impl SettlementService {
    /// Construct with the default economic parameters (tests, workers).
    pub fn new(pool: PgPool) -> Self {
        Self::with_econ(pool, EconParams::default())
    }

    /// Construct with a specific (e.g. config-loaded) parameter set.
    pub fn with_econ(pool: PgPool, econ: EconParams) -> Self {
        Self { pool, econ }
    }

    /// Settle one epoch. Idempotent: a second call for the same epoch is a no-op.
    ///
    /// Only CLOSED epochs settle. Settling the CURRENT (still-open) epoch would
    /// freeze it on a partial-day snapshot — the marker then makes every later
    /// interaction in that day unpayable — so it, the future, and negatives (which
    /// used to slip past the old `> current` guard and, via `as i32`, alias a real
    /// epoch) are all rejected.
    pub async fn settle(&self, epoch_k: i64) -> Result<SettlementSummary, ApiError> {
        if epoch_k < 0 {
            return Err(ApiError::Validation("epoch_negative"));
        }
        let current = Epoch::from_unix_seconds(Utc::now().timestamp()).0 as i64;
        if epoch_k >= current {
            return Err(ApiError::Validation("epoch_not_closed"));
        }
        let epoch_k_i32 =
            i32::try_from(epoch_k).map_err(|_| ApiError::Validation("epoch_out_of_range"))?;

        let interactions = InteractionRepository::new(self.pool.clone());
        let users = UserRepository::new(self.pool.clone());
        let gems = GemRepository::new(self.pool.clone());
        let params = &self.econ;
        let epoch = Epoch(epoch_k as u64);

        // Fast path FIRST, before the expensive graph build. The scheduler re-checks
        // a lookback window every tick; an already-settled epoch returns from the
        // (indexed) marker without ever loading edges or running PageRank.
        if let Some(m) = gems.settled_marker(epoch_k).await? {
            return Ok(SettlementSummary {
                epoch_k,
                emission: m.emission,
                user_count: m.user_count,
                already_settled: true,
            });
        }

        // 1. Resolved user→user edges for this epoch.
        let raw_edges = interactions.edges_for_epoch(epoch_k_i32).await?;

        // 2. Verified flags for every user that appears in an edge.
        let mut ids: BTreeSet<i64> = BTreeSet::new();
        for e in &raw_edges {
            ids.insert(e.actor_id);
            ids.insert(e.target_id);
        }
        let id_vec: Vec<i64> = ids.into_iter().collect();
        let flags = users.verified_flags(&id_vec).await?;

        let meta: Vec<UserMeta> = flags
            .into_iter()
            .map(|(id, verified)| UserMeta {
                user: UserId(id as u64),
                verified,
                account_burn_sats: 0,
            })
            .collect();
        // Recency-weight each interaction by e^(−λ·τ): τ = the event's age in days
        // at the epoch's CLOSE (deterministic, not wall-clock, so settlement stays
        // replayable), so newer interactions count for more. Applied here at the
        // seam — the engine stays pure on the weights it's given. λ = 0 ⇒ no decay,
        // recovering the prior behaviour. (Effect is intra-epoch and mild while
        // settlement is daily; the λ half-life bites once windows span days.)
        let epoch_end_secs = ((epoch_k + 1) * 86_400) as f64;
        let edges: Vec<Edge> = raw_edges
            .iter()
            .map(|e| {
                let tau_days =
                    ((epoch_end_secs - e.created_at.timestamp() as f64) / 86_400.0).max(0.0);
                Edge {
                    actor: UserId(e.actor_id as u64),
                    target: UserId(e.target_id as u64),
                    weight: e.weight * recency_factor(params.time_decay_lambda, tau_days),
                }
            })
            .collect();

        // 3. Build inputs and decide eligibility.
        let inputs = build_user_inputs(&edges, &meta, params);
        let user_count = inputs.len() as i32;
        let has_participants = inputs.iter().any(|i| i.verified);
        // The epoch's mint amount. The CALLER decides the source: Phase 1a uses the
        // fixed-schedule emission (points); v6 Phase 1b swaps in the demand-gated
        // mint `(1 − skim) · advertiser_inflow` HERE (ADR 0007), with no change to
        // the settlement worker that distributes it.
        let emission_pt = if has_participants {
            emission_for(epoch, params)
        } else {
            PtAmount(0)
        };
        // Stored on the marker as i64; checked so a future param change can't wrap
        // the conserved amount on the u128 → i64 boundary.
        let emission = i64::try_from(emission_pt.0)
            .map_err(|_| ApiError::Internal("emission exceeds i64".into()))?;

        // 4. Distribute by weight. Conservation (Σ payouts == emission) is a pure,
        //    fail-closed check done BEFORE any DB write.
        let payouts = compute_payouts(&inputs, params, emission_pt);
        let distributed: u128 = payouts.values().map(|p| p.0).sum();
        if distributed != emission_pt.0 {
            return Err(ApiError::Internal(format!(
                "conservation: distributed {distributed} != emission {}",
                emission_pt.0
            )));
        }

        // 5. Mint the payouts AND write the settlement marker in ONE transaction,
        //    serialized against other settlers of this epoch by a transaction-scoped
        //    advisory lock. This is the core idempotency fix:
        //    - The advisory lock stops the scheduler and a manual settle (or two
        //      scheduler instances) from interleaving mints for the same epoch.
        //    - Mint + marker committing together means there is NEVER a committed
        //      state where an epoch is minted but unmarked. That state was the window
        //      a divergent retry — recomputing from since-changed inputs (a flipped
        //      bot-gate flag, a restored post) — could over-emit into. Now a crash
        //      before commit leaves nothing; a retry recomputes cleanly and conserves.
        //    The per-(epoch,user) unique index on `ledger_entries` remains as
        //    belt-and-suspenders. No global supply read is taken here: conservation
        //    above plus the atomic, idempotent mint pin the supply delta exactly,
        //    without the cross-epoch race the old `total_supply()` check introduced.
        let mut tx = self.pool.begin().await?;
        GemRepository::lock_epoch(&mut tx, epoch_k).await?;
        // Re-check under the lock: a peer settler may have finished while we computed.
        if GemRepository::is_settled_tx(&mut tx, epoch_k).await? {
            tx.rollback().await?;
            let m = gems.settled_marker(epoch_k).await?;
            let (emission, user_count) = m
                .map(|m| (m.emission, m.user_count))
                .unwrap_or((emission, user_count));
            return Ok(SettlementSummary {
                epoch_k,
                emission,
                user_count,
                already_settled: true,
            });
        }
        if has_participants {
            let payout_vec: Vec<(UserId, PtAmount)> =
                payouts.iter().map(|(u, a)| (*u, *a)).collect();
            ledger::mint_epoch_tx(&mut tx, epoch, &payout_vec).await?;
        }
        let first_time =
            GemRepository::claim_epoch_tx(&mut tx, epoch_k, emission, user_count).await?;
        tx.commit().await?;

        Ok(SettlementSummary {
            epoch_k,
            emission,
            user_count,
            already_settled: !first_time,
        })
    }

    /// Settle every CLOSED epoch in the window `[current_epoch - lookback,
    /// current_epoch - 1]` (the current epoch is still open, so it is excluded).
    /// Idempotent — safe to run every scheduler tick and to catch up epochs missed
    /// during downtime. Returns a summary per epoch, oldest first.
    pub async fn settle_closed_epochs(
        &self,
        current_epoch: i64,
        lookback: i64,
    ) -> Result<Vec<SettlementSummary>, ApiError> {
        let lookback = lookback.max(1);
        let start = (current_epoch - lookback).max(0);
        let mut summaries = Vec::new();
        for epoch_k in start..current_epoch {
            summaries.push(self.settle(epoch_k).await?);
        }
        Ok(summaries)
    }

    /// A user's current off-chain gem balance.
    pub async fn gem_balance(&self, user_id: i64) -> Result<GemBalance, ApiError> {
        let ledger = PgLedger::new(self.pool.clone());
        let balance = ledger.balance(UserId(user_id as u64)).await?.0 as i64;
        Ok(GemBalance { user_id, balance })
    }
}

/// Interaction recency weight `e^(−λ·τ)` for an event `τ` days old at epoch close
/// (Dossier §4.3 `Σ ω·e^(−λτ)`). Monotonically decreasing in age; `λ = 0 ⇒ 1`
/// (no decay). The `time_decay_lambda` default (0.099/day) is a ~7-day half-life.
fn recency_factor(lambda: f64, tau_days: f64) -> f64 {
    (-lambda * tau_days.max(0.0)).exp()
}

#[cfg(test)]
mod tests {
    use super::recency_factor;

    #[test]
    fn recency_factor_decays_with_age() {
        // Brand-new interaction → full weight; λ=0 → no decay regardless of age.
        assert!((recency_factor(0.099, 0.0) - 1.0).abs() < 1e-12);
        assert_eq!(recency_factor(0.0, 5.0), 1.0);
        // ~7-day half-life at the default λ.
        assert!((recency_factor(0.099, 7.0) - 0.5).abs() < 0.01);
        // Strictly less weight the older the interaction is.
        assert!(recency_factor(0.099, 10.0) < recency_factor(0.099, 1.0));
    }
}
