//! Postgres-backed `LedgerBackend` (Phase 1a). Persists gem/PT balances in the
//! `gem_balances` table. The economic core (gem-engine, settlement) is unaware of
//! it — it sees only the `LedgerBackend` trait, so Phase 1b swaps in the Solana
//! backing with no change above this line.
//!
//! Balances are stored as BIGINT; see migration 0002 for why i64 is sufficient.

use async_trait::async_trait;
use domain::{Epoch, PtAmount, UserId};
use sqlx::PgPool;

use crate::{LedgerBackend, LedgerError, Result};

pub struct PgLedger {
    pool: PgPool,
}

impl PgLedger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn backend(err: sqlx::Error) -> LedgerError {
    LedgerError::Backend(err.to_string())
}

#[async_trait]
impl LedgerBackend for PgLedger {
    async fn balance(&self, user: UserId) -> Result<PtAmount> {
        let balance = sqlx::query_scalar!(
            "SELECT balance FROM gem_balances WHERE user_id = $1",
            user.0 as i64
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(PtAmount(balance.unwrap_or(0) as u128))
    }

    async fn mint(&self, user: UserId, amount: PtAmount, _epoch: Epoch) -> Result<()> {
        sqlx::query!(
            r#"
            INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
            ON CONFLICT (user_id) DO UPDATE
            SET balance = gem_balances.balance + EXCLUDED.balance, updated_at = now()
            "#,
            user.0 as i64,
            amount.0 as i64
        )
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn mint_epoch(&self, epoch: Epoch, payouts: &[(UserId, PtAmount)]) -> Result<PtAmount> {
        // One transaction for the whole epoch: either every payout (balance + its
        // journal entry) commits, or none does. The partial unique index
        // (epoch_k, user_id) WHERE kind='mint' makes each mint idempotent, so a
        // retry after a crash credits only the users still missing.
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let mut minted: u128 = 0;
        for (user, amount) in payouts {
            if amount.0 == 0 {
                continue;
            }
            let entry = sqlx::query!(
                r#"
                INSERT INTO ledger_entries (user_id, epoch_k, kind, amount, ref_type, ref_id)
                VALUES ($1, $2, 'mint', $3, 'epoch', $2)
                ON CONFLICT (epoch_k, user_id) WHERE kind = 'mint' DO NOTHING
                "#,
                user.0 as i64,
                epoch.0 as i64,
                amount.0 as i64,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;

            // Only move the balance if THIS call recorded the mint (idempotency).
            if entry.rows_affected() == 1 {
                sqlx::query!(
                    r#"
                    INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
                    ON CONFLICT (user_id) DO UPDATE
                    SET balance = gem_balances.balance + EXCLUDED.balance, updated_at = now()
                    "#,
                    user.0 as i64,
                    amount.0 as i64,
                )
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
                minted += amount.0;
            }
        }
        tx.commit().await.map_err(backend)?;
        Ok(PtAmount(minted))
    }

    async fn burn(&self, user: UserId, amount: PtAmount, _epoch: Epoch) -> Result<()> {
        // The `balance >= $2` guard makes the debit atomic: zero rows affected
        // means insufficient funds (also backstopped by the CHECK constraint).
        let res = sqlx::query!(
            r#"
            UPDATE gem_balances SET balance = balance - $2, updated_at = now()
            WHERE user_id = $1 AND balance >= $2
            "#,
            user.0 as i64,
            amount.0 as i64
        )
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if res.rows_affected() == 0 {
            return Err(LedgerError::InsufficientBalance(user));
        }
        Ok(())
    }

    async fn transfer(
        &self,
        from: UserId,
        to: UserId,
        amount: PtAmount,
        _epoch: Epoch,
    ) -> Result<()> {
        // Debit and credit must be atomic — one transaction, rolled back on any error.
        let mut tx = self.pool.begin().await.map_err(backend)?;

        let debited = sqlx::query!(
            r#"
            UPDATE gem_balances SET balance = balance - $2, updated_at = now()
            WHERE user_id = $1 AND balance >= $2
            "#,
            from.0 as i64,
            amount.0 as i64
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if debited.rows_affected() == 0 {
            return Err(LedgerError::InsufficientBalance(from));
        }

        sqlx::query!(
            r#"
            INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
            ON CONFLICT (user_id) DO UPDATE
            SET balance = gem_balances.balance + EXCLUDED.balance, updated_at = now()
            "#,
            to.0 as i64,
            amount.0 as i64
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn total_supply(&self) -> Result<PtAmount> {
        let total = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(balance), 0)::bigint AS "total!" FROM gem_balances"#
        )
        .fetch_one(&self.pool)
        .await
        .map_err(backend)?;
        Ok(PtAmount(total as u128))
    }
}
