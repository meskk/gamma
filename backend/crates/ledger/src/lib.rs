//! The ledger seam — the single most important architectural boundary.
//!
//! The economic core (gem-engine, settlement) depends ONLY on `LedgerBackend`.
//! Phase 1a backs it with `OffChainLedger` (in-memory now, Postgres next) where
//! tokens are points with no value. Phase 1b swaps in a Solana-backed impl with
//! ZERO change to the math above it. This trait is why a tokenomics or backing
//! change cannot ripple into the platform. See docs/adr/0002-ledger-backend-seam.md.

use async_trait::async_trait;
use domain::{Epoch, PtAmount, UserId};
use std::collections::HashMap;
use std::sync::Mutex;
use thiserror::Error;

mod pg;
pub use pg::{burn_tx, credit_tx, debit_tx, mint_epoch_tx, PgLedger};

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("insufficient balance for user {0:?}")]
    InsufficientBalance(UserId),
    #[error("backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, LedgerError>;

/// What the economic core needs from any backing — off-chain or on Solana.
///
/// Deliberately minimal: mint is the ONLY way supply grows, and it is driven
/// solely by the settlement worker on the fixed emission schedule (invariant ii).
#[async_trait]
pub trait LedgerBackend: Send + Sync {
    async fn balance(&self, user: UserId) -> Result<PtAmount>;

    /// Mint the day's emission share to a user. Settlement-only; never coupled to burns.
    async fn mint(&self, user: UserId, amount: PtAmount, epoch: Epoch) -> Result<()>;

    /// Atomically mint a whole epoch's payouts. ALL-OR-NOTHING and idempotent per
    /// (epoch, user): a retry after a partial failure mints only the users still
    /// missing and can never double-credit. This is the settlement entry point —
    /// it is what makes epoch settlement crash-safe, replacing a loop of separate
    /// `mint` calls that could leave an epoch half-paid.
    ///
    /// Returns the total NEWLY minted this call (0 on a full idempotent replay),
    /// so the caller can assert supply grew by exactly that — an invariant that
    /// holds for a fresh settle, a partial resume, and a no-op replay alike.
    async fn mint_epoch(&self, epoch: Epoch, payouts: &[(UserId, PtAmount)]) -> Result<PtAmount>;

    /// Burn PT against a user account (deflationary). Skim routing happens a layer up.
    async fn burn(&self, user: UserId, amount: PtAmount, epoch: Epoch) -> Result<()>;

    async fn transfer(
        &self,
        from: UserId,
        to: UserId,
        amount: PtAmount,
        epoch: Epoch,
    ) -> Result<()>;

    /// Total PT supply — used by the supply-monotonicity invariant.
    async fn total_supply(&self) -> Result<PtAmount>;
}

/// Phase-1a in-memory backing. The Postgres impl replaces `Inner` with SQL but
/// keeps this exact trait surface, so nothing downstream changes.
#[derive(Default)]
pub struct OffChainLedger {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    balances: HashMap<UserId, u128>,
    supply: u128,
    /// (epoch, user) pairs already minted — the in-memory mirror of the journal's
    /// per-epoch mint uniqueness, so `mint_epoch` is idempotent here too.
    minted: std::collections::HashSet<(u64, UserId)>,
}

#[async_trait]
impl LedgerBackend for OffChainLedger {
    async fn balance(&self, user: UserId) -> Result<PtAmount> {
        let g = self.inner.lock().unwrap();
        Ok(PtAmount(*g.balances.get(&user).unwrap_or(&0)))
    }

    async fn mint(&self, user: UserId, amount: PtAmount, _epoch: Epoch) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        *g.balances.entry(user).or_default() += amount.0;
        g.supply += amount.0;
        Ok(())
    }

    async fn mint_epoch(&self, epoch: Epoch, payouts: &[(UserId, PtAmount)]) -> Result<PtAmount> {
        let mut g = self.inner.lock().unwrap();
        let mut minted = 0u128;
        for (user, amount) in payouts {
            if amount.0 == 0 || !g.minted.insert((epoch.0, *user)) {
                continue; // zero share, or already minted this epoch (idempotent)
            }
            *g.balances.entry(*user).or_default() += amount.0;
            g.supply += amount.0;
            minted += amount.0;
        }
        Ok(PtAmount(minted))
    }

    async fn burn(&self, user: UserId, amount: PtAmount, _epoch: Epoch) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        let bal = g.balances.entry(user).or_default();
        if *bal < amount.0 {
            return Err(LedgerError::InsufficientBalance(user));
        }
        *bal -= amount.0;
        g.supply -= amount.0;
        Ok(())
    }

    async fn transfer(
        &self,
        from: UserId,
        to: UserId,
        amount: PtAmount,
        _epoch: Epoch,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        let from_bal = *g.balances.get(&from).unwrap_or(&0);
        if from_bal < amount.0 {
            return Err(LedgerError::InsufficientBalance(from));
        }
        *g.balances.entry(from).or_default() -= amount.0;
        *g.balances.entry(to).or_default() += amount.0;
        Ok(())
    }

    async fn total_supply(&self) -> Result<PtAmount> {
        Ok(PtAmount(self.inner.lock().unwrap().supply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mint_increases_supply_and_balance() {
        let l = OffChainLedger::default();
        l.mint(UserId(1), PtAmount(1000), Epoch(0)).await.unwrap();
        assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(1000));
        assert_eq!(l.total_supply().await.unwrap(), PtAmount(1000));
    }

    #[tokio::test]
    async fn burn_below_balance_fails() {
        let l = OffChainLedger::default();
        l.mint(UserId(1), PtAmount(10), Epoch(0)).await.unwrap();
        assert!(l.burn(UserId(1), PtAmount(11), Epoch(0)).await.is_err());
    }
}
