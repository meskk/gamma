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
docker compose up -d           # Postgres + Redis + MinIO (object storage)
cargo test --all               # all tests (need the services running)
cargo run -p core-api          # http://localhost:8080/health
cargo run --bin transcode_worker  # async HLS transcode worker (separate process)
```
Builds/clippy work offline via the committed `.sqlx` cache (`SQLX_OFFLINE=true`);
tests/runtime need the running services. Set `DATABASE_URL` (see `.env.example`).

## Where things are

- `crates/domain` — shared newtypes (the seam-free middle).
- `crates/econ-params` — the knobs.
- `crates/ledger` — the LedgerBackend seam + off-chain impl.
- `crates/gem-engine` — pure weight/PageRank/apportionment math.
- `crates/settlement` — epoch worker + invariants.
- `crates/core-api` — axum HTTP surface.
- `crates/storage` — S3/MinIO client (presigned upload/download).
- `migrations/` — SQL (forward-only). `0001_init.sql` includes interaction_events.
- `ARCHITECTURE.md` — the fuller map and rationale.

## Current status & next steps (snapshot 2026-06-16)

Phase 1a is well underway. Everything below is built and green (tests + fmt +
clippy), each a committed checkpoint — see `git log` for the full progression.

Done:
- Core domains (handler→service→repository, Postgres via sqlx): users, posts,
  follows, cold-start feed (Appendix A.2 bounded candidate query), and
  append-only interaction-graph capture.
- Gem economy: `gem-engine` (graph → PageRank → log-space weights), `settlement`
  worker with fail-closed conservation invariants, Postgres-backed ledger
  (`PgLedger`), off-chain epoch settlement (`POST /epochs/:k/settle`,
  `GET /users/:id/gems`).
- Media: object storage (MinIO/S3, presigned direct upload/download), async
  HLS transcoding (Redis queue + `transcode_worker` binary), paid content
  unlock in PT (creator/company-fee/burn split via `econ-params`) with an
  access-controlled HLS manifest (402 until unlocked).
- Auth: register/login (argon2 + opaque bearer sessions), `AuthUser` extractor;
  all write/spend/paid-access endpoints derive identity from the session.
- Roles: `Role` enum + `AdminUser` extractor; `POST /epochs/:k/settle` is
  operator-only (401/403/200). See ADR 0004/0005.

Audit remediation (2026-06-16, all committed + green) — a multi-agent review
found the foundation strong but several launch-blocking gaps; fixed:
- **Bot gate wired + secured**: removed the unauthenticated `POST /users`
  self-verify hole; the gate is now operator-only (`PUT /users/:id/verification`).
  ADR 0005.
- **Media access control**: `GET /media/:id` now gates the raw URL by entitlement
  (owner/free/unlocked); finalize/transcode are owner-only — closed a paywall
  bypass + IDOR.
- **Anti-abuse**: per-(actor,type,epoch,target,post) interaction dedup; 256 KiB
  body limit; per-IP rate limit (tower_governor, in `main.rs` only).
- **Atomic, crash-safe settlement + money journal**: `ledger_entries` append-only
  journal; `mint_epoch` (atomic, idempotent per (epoch,user)); mint-then-mark so
  a crash can't under-pay; unlock routed through the ledger seam with the burn
  recorded. ADR 0004.

Audit follow-up also done (2026-06-16): self-scoped reads (`GET /users/:id/gems`,
`/users/:id/feed`) now require the session and are owner-or-operator (`Caller`
extractor); the emission schedule is integer-exact (per-year step, no f64 on the
conserved amount) with a checked u128→i64 cast and a 21M-cap test.

Next steps (rough priority):
1. **AI ingestion service** (`services/ingestion`, Python) — the Rust SEAM now
   exists (ADR 0006): new posts are offered on the `gamma:ingestion` Redis queue,
   and signals are written back via operator-only `PUT /posts/:id/signals` into
   `content_signals` (JSONB). Still to do: the Python consumer itself (on the Mac
   Studio), and wiring the feed ranker to read `content_signals` (deferred until
   the signal shape is settled — no speculative ranking yet).
2. **Frontend API contract**: generate the typed contract (ts-rs / OpenAPI) so the
   "types break the frontend at compile time" claim is real, before the frontend.
3. **Periodic settlement scheduler** (cron) so epochs settle automatically.
4. **Multi-bitrate HLS ladder** + prod HLS delivery decision (already past 1a MVP).
5. **Smaller, tracked**: apply `time_decay_lambda` (or document it deferred);
   add `/v1` API prefix; request-tracing/metrics; FKs on `interaction_events`;
   provision MinIO+Redis in CI so the payment/media tests run there.

Working style: deliberate, ONE reviewable step at a time; verify (tests + fmt +
clippy green) before moving on; commit each checkpoint. Tokenomics knobs are in
flux by design — keep them in `econ-params`, never hardcoded.
