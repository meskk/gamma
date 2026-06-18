-- Append-only money journal. `gem_balances` is a mutable running total and can't
-- answer "how did this balance get here", reconstruct supply, or be reconciled —
-- which the project's own "auditable" value requires.
--
-- The PRODUCTION supply-moving paths write one immutable row here: settlement
-- (`LedgerBackend::mint_epoch`) and the paid-content unlock (the `credit_tx` /
-- `debit_tx` / `burn_tx` primitives). NOTE: the bare `LedgerBackend::mint`/`burn`/
-- `transfer` trait methods are low-level helpers used only by ledger unit tests and
-- do NOT journal — production never calls them directly. (If they ever gain a
-- production caller, make them journal too, or route through mint_epoch / the tx
-- primitives.)
--
-- `amount` is SIGNED: positive for a credit/mint, negative for a debit/burn.
-- `user_id` is NULL only for a pure burn (destruction with no holder).
CREATE TABLE ledger_entries (
    id         BIGSERIAL PRIMARY KEY,
    user_id    BIGINT,
    epoch_k    BIGINT NOT NULL,
    kind       TEXT NOT NULL,        -- 'mint' | 'burn' | 'unlock_debit' | 'unlock_credit'
    amount     BIGINT NOT NULL,
    ref_type   TEXT,                 -- e.g. 'epoch', 'unlock'
    ref_id     BIGINT,               -- e.g. asset_id for an unlock
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A user is minted AT MOST ONCE per epoch. This gives settlement intrinsic
-- idempotency and crash-resumability at the ledger level, independent of the
-- epoch_settlements marker: a retry after a partial crash re-mints only the
-- users still missing, and can never double-credit.
CREATE UNIQUE INDEX ledger_entries_epoch_mint
    ON ledger_entries (epoch_k, user_id)
    WHERE kind = 'mint';

CREATE INDEX ledger_entries_user ON ledger_entries (user_id);
