-- Settlement marker per epoch (Phase 1a idempotency guard). The worker claims an
-- epoch here BEFORE minting; a re-run finds the row and skips, so an epoch is
-- never double-settled. NOTE: fully crash-safe idempotency (mint + marker in one
-- epoch-keyed transaction, the Dossier B.2 state machine) is a Phase-1b hardening
-- step. Claiming before minting biases toward under-payment (recoverable) over
-- double-minting (corrupts supply) if the worker dies mid-mint.
CREATE TABLE epoch_settlements (
    epoch_k    BIGINT PRIMARY KEY,
    emission   BIGINT NOT NULL,
    user_count INTEGER NOT NULL,
    settled_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
