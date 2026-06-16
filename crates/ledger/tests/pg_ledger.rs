//! Integration tests for the Postgres-backed ledger against a real database.
//! Same behavioural contract as the in-memory `OffChainLedger`.

use domain::{Epoch, PtAmount, UserId};
use ledger::{LedgerBackend, PgLedger};
use sqlx::PgPool;

#[sqlx::test(migrations = "../../migrations")]
async fn mint_accumulates_balance_and_supply(pool: PgPool) {
    let l = PgLedger::new(pool);
    l.mint(UserId(1), PtAmount(1000), Epoch(0)).await.unwrap();
    l.mint(UserId(1), PtAmount(500), Epoch(0)).await.unwrap();
    l.mint(UserId(2), PtAmount(250), Epoch(0)).await.unwrap();

    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(1500));
    assert_eq!(l.balance(UserId(2)).await.unwrap(), PtAmount(250));
    assert_eq!(l.total_supply().await.unwrap(), PtAmount(1750));
    // Unknown user reads as zero, not an error.
    assert_eq!(l.balance(UserId(99)).await.unwrap(), PtAmount(0));
}

#[sqlx::test(migrations = "../../migrations")]
async fn burn_below_balance_fails_and_otherwise_debits(pool: PgPool) {
    let l = PgLedger::new(pool);
    l.mint(UserId(1), PtAmount(10), Epoch(0)).await.unwrap();

    assert!(l.burn(UserId(1), PtAmount(11), Epoch(0)).await.is_err());
    l.burn(UserId(1), PtAmount(4), Epoch(0)).await.unwrap();
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(6));
}

#[sqlx::test(migrations = "../../migrations")]
async fn mint_epoch_is_idempotent_per_epoch(pool: PgPool) {
    let l = PgLedger::new(pool);
    let payouts = [(UserId(1), PtAmount(100)), (UserId(2), PtAmount(50))];

    l.mint_epoch(Epoch(7), &payouts).await.unwrap();
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(100));
    assert_eq!(l.total_supply().await.unwrap(), PtAmount(150));

    // Re-running the SAME epoch credits nothing more — the journal's per-epoch
    // mint uniqueness makes it idempotent (this is what makes a crashed
    // settlement safely re-runnable).
    l.mint_epoch(Epoch(7), &payouts).await.unwrap();
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(100));
    assert_eq!(l.total_supply().await.unwrap(), PtAmount(150));

    // A different epoch mints again.
    l.mint_epoch(Epoch(8), &[(UserId(1), PtAmount(10))])
        .await
        .unwrap();
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(110));
    assert_eq!(l.total_supply().await.unwrap(), PtAmount(160));
}

#[sqlx::test(migrations = "../../migrations")]
async fn transfer_is_atomic(pool: PgPool) {
    let l = PgLedger::new(pool);
    l.mint(UserId(1), PtAmount(100), Epoch(0)).await.unwrap();

    l.transfer(UserId(1), UserId(2), PtAmount(40), Epoch(0))
        .await
        .unwrap();
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(60));
    assert_eq!(l.balance(UserId(2)).await.unwrap(), PtAmount(40));

    // Over-transfer fails and leaves balances untouched.
    assert!(l
        .transfer(UserId(1), UserId(2), PtAmount(1000), Epoch(0))
        .await
        .is_err());
    assert_eq!(l.balance(UserId(1)).await.unwrap(), PtAmount(60));
    assert_eq!(l.balance(UserId(2)).await.unwrap(), PtAmount(40));
}
