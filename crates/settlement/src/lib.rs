//! Daily settlement at the epoch boundary — idempotent and fail-closed.
//!
//! Orchestrates the epoch close: compute weights → mint the fixed emission →
//! distribute by weight → (Phase 1b: advertiser buy-and-burn, account/post burns,
//! publish to Solana). It depends only on `LedgerBackend`, so the SAME code runs
//! off-chain in 1a and on Solana in 1b.
//!
//! Every epoch asserts the conservation invariants and fails closed if any breaks
//! (Dossier App. B.2, six invariants). This scaffold implements the two that the
//! off-chain ledger can already check; the LP/escrow ones land with the 1b backing.

use domain::{Epoch, PtAmount};
use econ_params::EconParams;
use gem_engine::{compute_payouts, UserInputs};
use ledger::LedgerBackend;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("invariant violated: {0}")]
    Invariant(String),
    #[error(transparent)]
    Ledger(#[from] ledger::LedgerError),
}

/// Emission for an epoch on the FIXED schedule — never a function of burns or
/// advertiser spend (invariant ii: emission independence). Daily decay derived
/// from the 10%/yr annual taper.
pub fn emission_for(epoch: Epoch, params: &EconParams) -> PtAmount {
    let years = epoch.0 as f64 / 365.0;
    let decay = (1.0 - params.emission_decay_bps as f64 / 10_000.0).powf(years);
    PtAmount((params.emission_day0_pt as f64 * decay) as u128)
}

/// Settle one epoch from snapshotted inputs. Returns once the ledger reflects the
/// epoch's emission, or errors (fail-closed) if any checked invariant breaks.
pub async fn settle_epoch<L: LedgerBackend>(
    ledger: &L,
    epoch: Epoch,
    inputs: &[UserInputs],
    params: &EconParams,
) -> Result<(), SettlementError> {
    let supply_before = ledger.total_supply().await?.0;
    let emission = emission_for(epoch, params);

    let payouts = compute_payouts(inputs, params, emission);

    // Invariant (i) Conservation: Σ payouts == emission exactly.
    let distributed: u128 = payouts.values().map(|p| p.0).sum();
    if distributed != emission.0 {
        return Err(SettlementError::Invariant(format!(
            "conservation: distributed {distributed} != emission {}",
            emission.0
        )));
    }

    for (user, amount) in &payouts {
        if amount.0 > 0 {
            ledger.mint(*user, *amount, epoch).await?;
        }
    }

    // Invariant (iii) Supply monotonicity: supply grew by EXACTLY the emission.
    let supply_after = ledger.total_supply().await?.0;
    if supply_after != supply_before + emission.0 {
        return Err(SettlementError::Invariant(format!(
            "supply monotonicity: {supply_before} + {} != {supply_after}",
            emission.0
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::UserId;
    use ledger::OffChainLedger;

    fn sample_inputs(n: u64) -> Vec<UserInputs> {
        (0..n)
            .map(|i| UserInputs {
                user: UserId(i),
                verified: true,
                interaction_volume: i as f64 + 1.0,
                node_score: 0.1 * (i as f64 + 1.0),
                unique_factor: i as f64,
                audience: i as f64,
                account_burn_sats: 0,
            })
            .collect()
    }

    #[tokio::test]
    async fn settles_and_conserves() {
        let ledger = OffChainLedger::default();
        let params = EconParams::default();
        settle_epoch(&ledger, Epoch(0), &sample_inputs(10), &params)
            .await
            .unwrap();
        assert_eq!(
            ledger.total_supply().await.unwrap(),
            emission_for(Epoch(0), &params)
        );
    }

    #[tokio::test]
    async fn emission_decays_over_years() {
        let params = EconParams::default();
        let y0 = emission_for(Epoch(0), &params).0;
        let y1 = emission_for(Epoch(365), &params).0;
        assert!(y1 < y0, "emission must taper year over year");
    }

    /// End-to-end pure pipeline: interaction edges → build_user_inputs → settle.
    /// Proves the whole gem path (graph → weights → ledger) conserves emission and
    /// rewards a well-connected user over an isolated one.
    #[tokio::test]
    async fn settles_from_interaction_graph() {
        use gem_engine::{build_user_inputs, Edge, UserMeta};

        // Users 1,2,3 all engage user 1 (a hub); user 4 is verified but isolated.
        let edges = vec![
            Edge {
                actor: UserId(2),
                target: UserId(1),
                weight: 5.0,
            },
            Edge {
                actor: UserId(3),
                target: UserId(1),
                weight: 3.0,
            },
            Edge {
                actor: UserId(1),
                target: UserId(2),
                weight: 1.0,
            },
        ];
        let meta = vec![
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
            UserMeta {
                user: UserId(4),
                verified: true,
                account_burn_sats: 0,
            },
        ];
        let params = EconParams::default();
        let inputs = build_user_inputs(&edges, &meta, &params);

        let ledger = OffChainLedger::default();
        settle_epoch(&ledger, Epoch(0), &inputs, &params)
            .await
            .unwrap();

        // Conservation: total minted == the epoch's emission.
        assert_eq!(
            ledger.total_supply().await.unwrap(),
            emission_for(Epoch(0), &params)
        );

        // The hub (user 1) out-earns the isolated user (user 4).
        let hub = ledger.balance(UserId(1)).await.unwrap().0;
        let isolated = ledger.balance(UserId(4)).await.unwrap().0;
        assert!(
            hub > isolated,
            "well-connected user should earn more than an isolated one"
        );
    }
}
