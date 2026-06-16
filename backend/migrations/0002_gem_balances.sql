-- Off-chain gem / PT balances (Phase 1a). One row per user; balance in PT base
-- units. BIGINT suffices: the 21M PEER cap in base units (×1e9) = 2.1e16, well
-- within i64's ~9.2e18. The Phase-1b Solana ledger replaces this backing behind
-- the same LedgerBackend trait — no change to the gem engine or settlement.
CREATE TABLE gem_balances (
    user_id    BIGINT PRIMARY KEY,
    balance    BIGINT NOT NULL DEFAULT 0 CHECK (balance >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
