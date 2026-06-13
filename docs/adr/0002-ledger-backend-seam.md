# ADR 0002 — The LedgerBackend seam

Status: accepted · Date: 2026-06-13

## Context

The same Gamma weight math runs in Phase 1a (off-chain, no value) and Phase 1b
(real PT on Solana). The Dossier is explicit: "the math is identical; only the
ledger backing changes." The owner needs Phase-1a work to be fully reusable in
1b, and needs tokenomics/backing changes not to ripple into the platform.

## Decision

Define a single trait `LedgerBackend` (`crates/ledger`) with the minimal surface
the economic core needs: `balance`, `mint`, `burn`, `transfer`, `total_supply`.
`gem-engine` and `settlement` depend ONLY on this trait.

- Phase 1a: `OffChainLedger` (in-memory now → Postgres next).
- Phase 1b: `SolanaLedger` (SPL Token-2022 + constant-product LP), same surface.

## Rationale

`mint` is the only way supply grows, and only the settlement worker calls it on
the fixed emission schedule — this makes invariant (ii) "emission independence"
structurally enforced, not just checked. Swapping the backing is an
implementation change behind a stable interface; nothing above it moves.

## Consequences

- The off-chain and Solana backends must pass the same behavioral test suite.
- The settlement worker is a separate process/crate, so even a Go Core API
  (see ADR 0001 escape hatch) talks to settlement over a clean boundary.
