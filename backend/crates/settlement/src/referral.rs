//! Referral-cut redistribution (MASTERPLAN P-2): pure, conserving payout
//! post-processing, applied AFTER Hamilton apportionment and BEFORE minting.
//!
//! Invariants, by construction:
//! - CONSERVING: every cut is moved, never created — Σ payouts is unchanged,
//!   so settlement's conservation check holds before and after.
//! - ONE LEVEL: every cut is computed from the ORIGINAL payouts, so a cut a
//!   referrer receives is never re-cut by *their* referrer (no pyramids).
//! - FLOORED: `floor(original · bps / 10_000)` — the remainder stays with the
//!   referred user; a small payout can round the cut to zero.
//!
//! Eligibility (active window, verified referrer) is decided by the CALLER —
//! this module is pure math over what it is handed.

use std::collections::{BTreeMap, BTreeSet};

use domain::{PtAmount, UserId};

/// An eligible referral for one epoch: `referred`'s payout owes `referrer` a
/// `bps` cut (basis points, clamped to 100%).
#[derive(Debug, Clone, Copy)]
pub struct ReferralCut {
    pub referred: UserId,
    pub referrer: UserId,
    pub bps: u16,
}

/// One applied redirect, for journaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferralTransfer {
    pub referred: UserId,
    pub referrer: UserId,
    pub amount: PtAmount,
}

/// Apply the cuts to the payout map, returning the transfers that actually
/// moved value (zero-amount cuts are skipped). At most one cut per referred
/// user — the referrals table's primary key guarantees that upstream; a
/// duplicate handed in here is ignored rather than double-cut.
pub fn apply_referral_cuts(
    payouts: &mut BTreeMap<UserId, PtAmount>,
    cuts: &[ReferralCut],
) -> Vec<ReferralTransfer> {
    // Phase 1: compute every transfer from the ORIGINAL payouts (one-level rule).
    let mut seen: BTreeSet<UserId> = BTreeSet::new();
    let mut transfers: Vec<ReferralTransfer> = Vec::new();
    for cut in cuts {
        if cut.referred == cut.referrer || !seen.insert(cut.referred) {
            continue;
        }
        let Some(original) = payouts.get(&cut.referred).copied() else {
            continue;
        };
        let amount = original.0 * u128::from(cut.bps.min(10_000)) / 10_000;
        if amount == 0 {
            continue;
        }
        transfers.push(ReferralTransfer {
            referred: cut.referred,
            referrer: cut.referrer,
            amount: PtAmount(amount),
        });
    }

    // Phase 2: apply. Subtractions can't underflow (amount ≤ original by
    // construction, one cut per referred).
    for t in &transfers {
        if let Some(p) = payouts.get_mut(&t.referred) {
            p.0 -= t.amount.0;
        }
        payouts
            .entry(t.referrer)
            .and_modify(|p| p.0 += t.amount.0)
            .or_insert(t.amount);
    }
    transfers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(u64, u128)]) -> BTreeMap<UserId, PtAmount> {
        entries
            .iter()
            .map(|(u, a)| (UserId(*u), PtAmount(*a)))
            .collect()
    }

    fn total(m: &BTreeMap<UserId, PtAmount>) -> u128 {
        m.values().map(|p| p.0).sum()
    }

    #[test]
    fn conserves_the_total_and_pays_the_referrer() {
        let mut payouts = map(&[(1, 1_000), (2, 500)]);
        let before = total(&payouts);
        // user 9 (not in the map) referred user 1 at 10%.
        let transfers = apply_referral_cuts(
            &mut payouts,
            &[ReferralCut {
                referred: UserId(1),
                referrer: UserId(9),
                bps: 1_000,
            }],
        );
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].amount, PtAmount(100));
        assert_eq!(payouts[&UserId(1)], PtAmount(900));
        assert_eq!(payouts[&UserId(9)], PtAmount(100));
        assert_eq!(total(&payouts), before, "Σ payouts must be unchanged");
    }

    #[test]
    fn one_level_only_a_received_cut_is_never_recut() {
        // Chain: 3 referred 2, 2 referred 1. Cuts on 1 and 2 are both computed
        // from ORIGINAL payouts — 3's income from 2 is untouched by 3's own
        // (nonexistent) referrer, and 2's cut ignores what 2 receives from 1.
        let mut payouts = map(&[(1, 1_000), (2, 1_000)]);
        let before = total(&payouts);
        let transfers = apply_referral_cuts(
            &mut payouts,
            &[
                ReferralCut {
                    referred: UserId(1),
                    referrer: UserId(2),
                    bps: 1_000,
                },
                ReferralCut {
                    referred: UserId(2),
                    referrer: UserId(3),
                    bps: 1_000,
                },
            ],
        );
        assert_eq!(transfers.len(), 2);
        // 2's cut to 3 is 10% of 2's ORIGINAL 1000 — not of 1000 + the 100
        // received from 1.
        assert_eq!(payouts[&UserId(3)], PtAmount(100));
        assert_eq!(payouts[&UserId(2)], PtAmount(1_000 - 100 + 100));
        assert_eq!(payouts[&UserId(1)], PtAmount(900));
        assert_eq!(total(&payouts), before);
    }

    #[test]
    fn floors_to_zero_skips_duplicates_and_clamps_bps() {
        let mut payouts = map(&[(1, 33), (2, 100)]);
        let before = total(&payouts);
        let transfers = apply_referral_cuts(
            &mut payouts,
            &[
                // 3% of 33 floors to 0 → no transfer, no journal noise.
                ReferralCut {
                    referred: UserId(1),
                    referrer: UserId(9),
                    bps: 300,
                },
                // Duplicate referred user: first entry wins, this one is ignored.
                ReferralCut {
                    referred: UserId(1),
                    referrer: UserId(8),
                    bps: 5_000,
                },
                // bps beyond 100% clamps: at most the full payout moves.
                ReferralCut {
                    referred: UserId(2),
                    referrer: UserId(9),
                    bps: 60_000,
                },
            ],
        );
        assert_eq!(transfers.len(), 1);
        assert_eq!(payouts[&UserId(2)], PtAmount(0));
        assert_eq!(payouts[&UserId(9)], PtAmount(100));
        assert_eq!(total(&payouts), before);
    }

    #[test]
    fn self_referral_and_unknown_referred_are_ignored() {
        let mut payouts = map(&[(1, 1_000)]);
        let transfers = apply_referral_cuts(
            &mut payouts,
            &[
                ReferralCut {
                    referred: UserId(1),
                    referrer: UserId(1),
                    bps: 1_000,
                },
                // Referred user earned nothing this epoch.
                ReferralCut {
                    referred: UserId(7),
                    referrer: UserId(9),
                    bps: 1_000,
                },
            ],
        );
        assert!(transfers.is_empty());
        assert_eq!(payouts[&UserId(1)], PtAmount(1_000));
    }
}
