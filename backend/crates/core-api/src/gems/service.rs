//! The epoch settlement orchestration: edges + verified flags → gem-engine inputs
//! → settle into the Postgres ledger, guarded for idempotency.

use std::collections::BTreeSet;

use db::PgPool;
use domain::{Epoch, PtAmount, UserId};
use gem_engine::{build_user_inputs, Edge, UserMeta};
use ledger::{LedgerBackend, PgLedger};
use settlement::{emission_for, settle_epoch};

use crate::error::ApiError;
use crate::gems::model::{GemBalance, SettlementSummary};
use crate::gems::repository::GemRepository;
use crate::interactions::repository::InteractionRepository;
use crate::users::repository::UserRepository;

#[derive(Clone)]
pub struct SettlementService {
    pool: PgPool,
}

impl SettlementService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Settle one epoch. Idempotent: a second call for the same epoch is a no-op.
    pub async fn settle(&self, epoch_k: i64) -> Result<SettlementSummary, ApiError> {
        let interactions = InteractionRepository::new(self.pool.clone());
        let users = UserRepository::new(self.pool.clone());
        let gems = GemRepository::new(self.pool.clone());
        let ledger = PgLedger::new(self.pool.clone());
        // Calibration defaults for now; later loaded from the versioned econ-params.
        let params = econ_params::EconParams::default();
        let epoch = Epoch(epoch_k as u64);

        // 1. Resolved user→user edges for this epoch.
        let raw_edges = interactions.edges_for_epoch(epoch_k as i32).await?;

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
        let edges: Vec<Edge> = raw_edges
            .iter()
            .map(|e| Edge {
                actor: UserId(e.actor_id as u64),
                target: UserId(e.target_id as u64),
                weight: e.weight,
            })
            .collect();

        // 3. Build inputs and decide eligibility.
        let inputs = build_user_inputs(&edges, &meta, &params);
        let user_count = inputs.len() as i32;
        let has_participants = inputs.iter().any(|i| i.verified);
        // The epoch's mint amount. The CALLER decides the source: Phase 1a uses the
        // fixed-schedule emission (points); v6 Phase 1b swaps in the demand-gated
        // mint `(1 − skim) · advertiser_inflow` HERE (ADR 0007), with no change to
        // the settlement worker that distributes it.
        let emission_pt = if has_participants {
            emission_for(epoch, &params)
        } else {
            PtAmount(0)
        };
        // Stored on the marker as i64; checked so a future param change can't wrap
        // the conserved amount on the u128 → i64 boundary.
        let emission = i64::try_from(emission_pt.0)
            .map_err(|_| ApiError::Internal("emission exceeds i64".into()))?;

        // 4. Fast path: an epoch already settled needs no work.
        if gems.is_settled(epoch_k).await? {
            return Ok(SettlementSummary {
                epoch_k,
                emission,
                user_count,
                already_settled: true,
            });
        }

        // 5. Mint by weight FIRST (atomic + idempotent per (epoch, user) at the
        //    ledger level), THEN record the settlement marker. Minting before the
        //    marker means a crash mid-settlement leaves NO marker, so a retry
        //    re-mints idempotently and completes — the marker can never flag an
        //    under-paid epoch as done. (Earlier this was claim-then-mint, which on
        //    a mid-mint crash permanently under-paid the epoch.)
        if has_participants {
            settle_epoch(&ledger, epoch, &inputs, &params, emission_pt).await?;
        }
        let first_time = gems.claim_epoch(epoch_k, emission, user_count).await?;

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
