# ADR 0004 — Append-only ledger journal + atomic, crash-safe settlement

Status: accepted · Date: 2026-06-16

## Context

A multi-perspective audit (2026-06-16) found two linked correctness gaps in the
money path:

1. **Non-atomic settlement.** The epoch marker (`epoch_settlements`) was claimed
   in one transaction, then the per-user mints ran in separate auto-committed
   statements. A crash mid-mint left the epoch permanently marked settled but
   only partially paid, with no recovery path — a retry saw the marker and
   minted nothing. (High severity.)
2. **No money journal.** `gem_balances` was a mutable running total only: no
   audit trail, no way to reconstruct supply at an epoch, no reconciliation —
   contradicting the project's own "auditable" value. The paid-content unlock
   also moved PT with raw SQL and silently dropped the burned amount.

## Decision

- Add an append-only `ledger_entries` journal (migration 0010). Every supply
  mutation (mint, unlock debit/credit, burn) writes one immutable row. `amount`
  is signed; a pure burn has `user_id = NULL`. A partial unique index
  `(epoch_k, user_id) WHERE kind = 'mint'` makes each mint idempotent.
- Add `LedgerBackend::mint_epoch(epoch, payouts)`: mints a whole epoch in ONE
  transaction, idempotent per `(epoch, user)`, returning the amount NEWLY minted.
  `settle_epoch` uses it and asserts supply grew by exactly that amount (holds for
  a fresh settle, a partial resume, and a no-op replay).
- Settlement now mints BEFORE recording the marker. A crash leaves no marker, so
  a retry re-mints idempotently and completes; the marker can never flag an
  under-paid epoch as done.
- All gem-balance mutation goes through the ledger crate — including the unlock,
  via transaction-scoped `debit_tx`/`credit_tx`/`burn_tx` primitives the media
  repository composes within its single transaction. See [[0002-ledger-backend-seam]].

## Rationale

Idempotency belongs at the ledger (the journal's per-epoch mint uniqueness),
not bolted onto an upstream marker that can desynchronize from the actual mints.
"Mint, then mark" plus an idempotent `mint_epoch` makes settlement crash-safe
off-chain now, rather than deferring it to Phase 1b. The journal is the table
that makes 1a reconciliation and 1b auditability possible at all.

## Consequences

- A second swap path no longer exists: content unlocks and settlement both move
  PT through the ledger crate, so the Phase-1b backing swaps in one place.
- The marker is now metadata + a fast-path, not the idempotency mechanism.
- Not yet addressed (tracked separately): emission is still computed in f64
  (a conserved quantity); balances are still stored as `BIGINT` and could be
  derived from the journal as a materialized sum in a later step.
