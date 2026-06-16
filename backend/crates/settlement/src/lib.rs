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
/// advertiser spend (invariant ii: emission independence).
///
/// Computed in INTEGER arithmetic only: no float touches this conserved quantity,
/// so the minted total is bit-stable across platforms. The 10%/yr taper is a
/// per-YEAR geometric step — emission is constant within a year and multiplied by
/// `(10000 - emission_decay_bps)/10000` at each year boundary. (A continuous daily
/// decay would require an irrational power and so couldn't be integer-exact.)
pub fn emission_for(epoch: Epoch, params: &EconParams) -> PtAmount {
    let years = epoch.0 / 365;
    let keep = 10_000u128.saturating_sub(params.emission_decay_bps as u128);
    let mut emission = params.emission_day0_pt as u128;
    for _ in 0..years {
        emission = emission * keep / 10_000;
        if emission == 0 {
            break; // fully tapered; no point looping further
        }
    }
    PtAmount(emission)
}

/// Settle one epoch from snapshotted inputs, minting `emission` PT and
/// distributing it by weight. Returns once the ledger reflects it, or errors
/// (fail-closed) if any checked invariant breaks.
///
/// `emission` is supplied by the CALLER — the settlement worker distributes the
/// pot, it does not decide its size. The source is a phase/economics decision held
/// outside this function: Phase 1a (points) passes the fixed-schedule
/// `emission_for(epoch, params)`; v6 Phase 1b will pass the demand-gated mint
/// `(1 − skim) · advertiser_inflow` (ADR 0007). This keeps the v5→v6 emission
/// change a drop-in at the call site, not an edit here.
pub async fn settle_epoch<L: LedgerBackend>(
    ledger: &L,
    epoch: Epoch,
    inputs: &[UserInputs],
    params: &EconParams,
    emission: PtAmount,
) -> Result<(), SettlementError> {
    let supply_before = ledger.total_supply().await?.0;

    let payouts = compute_payouts(inputs, params, emission);

    // Invariant (i) Conservation: Σ payouts == emission exactly.
    let distributed: u128 = payouts.values().map(|p| p.0).sum();
    if distributed != emission.0 {
        return Err(SettlementError::Invariant(format!(
            "conservation: distributed {distributed} != emission {}",
            emission.0
        )));
    }

    // Mint the whole epoch atomically: all payouts commit together or none do,
    // and a retry after a crash completes only the missing mints (idempotent).
    // This replaces a loop of separate, individually-committed mints that could
    // leave an epoch half-paid. `minted` is what was NEWLY minted this call (0 on
    // a full replay), which is what the supply invariant below checks against.
    let payout_vec: Vec<_> = payouts
        .iter()
        .map(|(user, amount)| (*user, *amount))
        .collect();
    let minted = ledger.mint_epoch(epoch, &payout_vec).await?;

    // Invariant (iii) Supply monotonicity: supply grew by EXACTLY what was minted.
    // Conservation (above) already pins the epoch's full payout to the emission;
    // this catches any supply the ledger added or dropped during minting, and
    // holds for a fresh settle, a partial resume, and a no-op replay alike.
    let supply_after = ledger.total_supply().await?.0;
    if supply_after != supply_before + minted.0 {
        return Err(SettlementError::Invariant(format!(
            "supply monotonicity: {supply_before} + {} != {supply_after}",
            minted.0
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
        let emission = emission_for(Epoch(0), &params);
        settle_epoch(&ledger, Epoch(0), &sample_inputs(10), &params, emission)
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

    #[test]
    fn emission_is_integer_exact_per_year_step() {
        let params = EconParams::default();
        let day0 = params.emission_day0_pt as u128;
        // Constant within a year; an exact integer 10% step at the year boundary.
        assert_eq!(emission_for(Epoch(0), &params).0, day0);
        assert_eq!(
            emission_for(Epoch(364), &params).0,
            day0,
            "emission is constant within a year"
        );
        assert_eq!(emission_for(Epoch(365), &params).0, day0 * 9_000 / 10_000);
        assert_eq!(
            emission_for(Epoch(730), &params).0,
            day0 * 9_000 / 10_000 * 9_000 / 10_000
        );
    }

    #[test]
    fn cumulative_emission_stays_under_21m_cap() {
        let params = EconParams::default();
        // Sum the entire schedule (emission floors to 0 within ~130 years) and
        // assert it never exceeds the 21M PEER hard cap. This catches a future
        // econ-params change that would silently breach the cap.
        let cap = 21_000_000u128 * domain::PT_ONE;
        let mut total = 0u128;
        for year in 0..200u64 {
            // Emission is constant within a year, so annual = 365 × the day rate.
            total += emission_for(Epoch(year * 365), &params).0 * 365;
        }
        assert!(
            total < cap,
            "cumulative emission {total} must stay under the 21M cap {cap}"
        );
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
        let emission = emission_for(Epoch(0), &params);
        settle_epoch(&ledger, Epoch(0), &inputs, &params, emission)
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
