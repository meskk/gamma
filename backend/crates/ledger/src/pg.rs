//! Postgres-backed `LedgerBackend` (Phase 1a). Persists gem/PT balances in the
//! `gem_balances` table. The economic core (gem-engine, settlement) is unaware of
//! it — it sees only the `LedgerBackend` trait, so Phase 1b swaps in the Solana
//! backing with no change above this line.
//!
//! Balances are stored as BIGINT; see migration 0002 for why i64 is sufficient.

use async_trait::async_trait;
use domain::{Epoch, PtAmount, UserId};
use sqlx::{PgConnection, PgPool};

use crate::{LedgerBackend, LedgerError, Result};

// --- Transaction-scoped money primitives (Phase 1a, Postgres) -----------------
//
// These move PT *inside a transaction the caller owns*, journaling every leg, so
// a multi-leg operation (e.g. a paid content unlock) stays atomic AND goes through
// the ledger crate rather than hand-written SQL elsewhere. They keep `gem_balances`
// mutation in ONE place — the seam — so Phase 1b can swap the backing here. They
// work in DB-native i64 because the unlock split is computed in i64 upstream.

/// Credit a user and journal it, within the caller's transaction.
pub async fn credit_tx(
    conn: &mut PgConnection,
    user_id: i64,
    amount: i64,
    epoch_k: i64,
    kind: &str,
    ref_id: i64,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO ledger_entries (user_id, epoch_k, kind, amount, ref_type, ref_id)
           VALUES ($1, $2, $3, $4, 'unlock', $5)"#,
        user_id,
        epoch_k,
        kind,
        amount,
        ref_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"INSERT INTO gem_balances (user_id, balance) VALUES ($1, $2)
           ON CONFLICT (user_id) DO UPDATE
           SET balance = gem_balances.balance + EXCLUDED.balance, updated_at = now()"#,
        user_id,
        amount
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Debit a user (guarded) and journal it. Returns `false` if the balance is too
/// low — the caller should abort (roll back) the transaction.
pub async fn debit_tx(
    conn: &mut PgConnection,
    user_id: i64,
    amount: i64,
    epoch_k: i64,
    kind: &str,
    ref_id: i64,
) -> std::result::Result<bool, sqlx::Error> {
    let debited = sqlx::query!(
        r#"UPDATE gem_balances SET balance = balance - $2, updated_at = now()
           WHERE user_id = $1 AND balance >= $2"#,
        user_id,
        amount
    )
    .execute(&mut *conn)
    .await?;
    if debited.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query!(
        r#"INSERT INTO ledger_entries (user_id, epoch_k, kind, amount, ref_type, ref_id)
           VALUES ($1, $2, $3, $4, 'unlock', $5)"#,
        user_id,
        epoch_k,
        kind,
        -amount,
        ref_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(true)
}

/// Mint an epoch's payouts within the caller's transaction, journaling each and
/// crediting balances. Idempotent per (epoch, user) via the partial unique index
/// on `ledger_entries` — a retry credits only users still missing. Returns the
/// amount NEWLY minted by THIS call (0 on a full replay).
///
/// Exposing the transaction-scoped form lets the caller commit the mint and the
/// settlement marker in ONE transaction, so there is never a committed state where
/// an epoch is minted but unmarked — the window a divergent retry (recomputed from
/// changed inputs) could over-emit into.
pub async fn mint_epoch_tx(
    conn: &mut PgConnection,
    epoch: Epoch,
    payouts: &[(UserId, PtAmount)],
) -> std::result::Result<PtAmount, sqlx::Error> {
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
        .execute(&mut *conn)
        .await?;

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
            .execute(&mut *conn)
            .await?;
            minted += amount.0;
        }
    }
    Ok(PtAmount(minted))
}

/// Journal an applied referral cut, within the caller's transaction. No balance
/// moves here — the epoch's mint rows already carry the POST-cut amounts; this
/// entry documents WHY the referred user's mint is smaller than their computed
/// share (`user_id` = the referrer who received the cut, `ref_id` = the
/// referred user it came from). Balance reconstruction must therefore ignore
/// 'referral' rows, exactly like 'unlock_burn'.
pub async fn referral_cut_tx(
    conn: &mut PgConnection,
    referrer_id: i64,
    referred_id: i64,
    amount: i64,
    epoch_k: i64,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO ledger_entries (user_id, epoch_k, kind, amount, ref_type, ref_id)
           VALUES ($1, $2, 'referral', $3, 'referred_user', $4)"#,
        referrer_id,
        epoch_k,
        amount,
        referred_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Record a burn (destruction with no holder) for audit, within the caller's
/// transaction. No balance moves — the destroyed amount is the part of a debit
/// that was never credited; this entry makes that destruction explicit. It has
/// `user_id = NULL`, so balance reconstruction (sum over non-null users) excludes
/// it and isn't double-counted.
pub async fn burn_tx(
    conn: &mut PgConnection,
    amount: i64,
    epoch_k: i64,
    ref_id: i64,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query!(
        r#"INSERT INTO ledger_entries (user_id, epoch_k, kind, amount, ref_type, ref_id)
           VALUES (NULL, $1, 'unlock_burn', $2, 'unlock', $3)"#,
        epoch_k,
        -amount,
        ref_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

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
        // journal entry) commits, or none does. Delegates to `mint_epoch_tx` so the
        // pool-owned path and the caller-owned (mint + marker atomic) path share one
        // implementation.
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let minted = mint_epoch_tx(&mut tx, epoch, payouts)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(minted)
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
