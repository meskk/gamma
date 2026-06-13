# CLAUDE.md — context for AI sessions

This file is auto-loaded by Claude Code at the start of every session. Read it
first. It exists so that any future AI session (or human) understands what this
project is and how it is built without re-deriving it from scratch.

## What this project is

A rebuild of **Peer Network**, a social platform implementing the **Gamma**
protocol: a social-mining economy where genuine social interaction (not
proof-of-work) earns a daily-minted token (PT / "PEER"). The authoritative
planning document is `Peer Network — Consolidated Rebuild Dossier v5.pdf` (not
in this repo; in the owner's Downloads). Key facts:

- **Daily epochs** mint PT on a fixed schedule (21M cap, 10%/yr taper) and
  distribute it by a **log-space social-weight score** (PageRank + concave burn
  multiplier + audience term, gated by a hard bot gate).
- **Advertisers** are the primary money inflow: they buy PT and burn it (98%
  destroyed, 2% to the company). PT floats against BTC on a constant-product LP.
- **Theorem 8.1:** mean user income ≤ advertiser spend. Redistribution can't
  enlarge the pie.

## Phases (build order)

- **Phase 1a** — off-chain social product, NO real money. Gems are points. This
  is what we build first. (Core API, AI ingestion, cold-start feed, gem math
  against an off-chain ledger.)
- **Phase 1a-β** — small closed beta with capped real cash-out value.
- **Phase 1b** — real money on Solana (SPL token, LP, custody, advertiser API).
  GATED by Gate A (legal) + Gate B (audit). Not date-committed.
- **Phase 2** — scale, ZK targeting, formal KYC, user wallets, chat.

## The one architectural idea that matters most

**The economic layer is decoupled from its backing and from its parameters**, so
tokenomics changes (which the owner says WILL happen) do not ripple into the
platform:

1. `crates/ledger` defines the `LedgerBackend` trait. Phase 1a = `OffChainLedger`;
   Phase 1b = a Solana-backed impl. The math above never changes.
2. `crates/econ-params` holds EVERY economic knob as versioned config. Never
   hardcode an economic constant anywhere else.
3. `crates/gem-engine` is pure, deterministic math. `crates/settlement`
   orchestrates the epoch and asserts conservation invariants (fail-closed).

When changing economics, change `econ-params` (bump `version`) and/or the
`LedgerBackend` impl — not the engine, not the API.

## Stack

- **Backend: Rust** (single Cargo workspace). axum + tokio + sqlx (Postgres).
  Money-critical core (gem-engine, settlement, ledger) is Rust for determinism,
  integer-exact math, no GC pauses, and auditability. See `docs/adr/0001`.
- **AI ingestion: Python** (`services/ingestion`, added later) on a Mac Studio.
- **Frontend: Next.js + TypeScript** (separate repo, added later). Types are
  generated from the Rust API so backend changes break the frontend at compile
  time, not at runtime.
- **DB: Postgres** (source of truth), **Redis** (queue/cache).

## Conventions

- Layering in services: `handler → service → repository`. Follow it everywhere.
- All money is integer fixed-point (`Sats`, `PtAmount`). **No floats on conserved
  quantities.** Floats are only allowed inside weight scoring, never in payouts.
- Payout apportionment is largest-remainder (Hamilton) so sums are exact.
- Settlement must be idempotent and epoch-keyed (a retry must never double-mint).
- One responsibility per crate; a `//!` doc at the top of each `lib.rs` explains it.
- Decisions are recorded in `docs/adr/`. Add an ADR when you make an architectural
  call; read them to understand why things are the way they are.

## How to run

```sh
docker compose up -d        # Postgres + Redis
cargo test --all            # includes the settlement invariant tests
cargo run -p core-api       # http://localhost:8080/health
```

## Where things are

- `crates/domain` — shared newtypes (the seam-free middle).
- `crates/econ-params` — the knobs.
- `crates/ledger` — the LedgerBackend seam + off-chain impl.
- `crates/gem-engine` — pure weight/PageRank/apportionment math.
- `crates/settlement` — epoch worker + invariants.
- `crates/core-api` — axum HTTP surface.
- `migrations/` — SQL (forward-only). `0001_init.sql` includes interaction_events.
- `ARCHITECTURE.md` — the fuller map and rationale.
