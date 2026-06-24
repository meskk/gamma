# CLAUDE.md — context for AI sessions

This file is auto-loaded by Claude Code at the start of every session. Read it
first. It exists so that any future AI session (or human) understands what this
project is and how it is built without re-deriving it from scratch.

## What this project is

A rebuild of **Peer Network**, an ad-and-creativity-funded social platform that
pays its users, implementing the **Gamma** protocol. The authoritative planning
document is **`Peer Network — Consolidated Rebuild Dossier v6 (Pol B).md`**
(`~/Desktop/Dokuwar/`, not in this repo). **The project is committed to the v6
economic spine; it supersedes v5 — see ADR 0007.** Key facts:

- **Two money rails.** (1) A **creative marketplace**: users sell/gate content and
  tip each other, paid in stablecoin (USDC); the company takes a marketplace fee
  (~10% gross). Live from day one; no token. (2) An **ad-revenue token (PEER)**:
  when advertisers pay, the platform mints a **fully-backed, DEMAND-GATED** token
  against that realized money (no fixed schedule, no cap; `A_d = 0 ⇒ no mint`,
  2% company skim) and distributes it by a **log-space social-weight score**
  (PageRank + concave burn multiplier + audience term, gated by a hard bot gate
  `v_i`). PEER is **non-tradable closed-loop credit**, redeemable 1:1 from the
  reserve — no LP, no float, no genesis seed.
- **The honesty floor (`[proven]`):** PEER in circulation ≤ reserve at every
  epoch's end (mint only against money already in reserve). Conservation defeats
  the closed-loop self-dealer for free but is **SILENT on the open-loop bot
  harvest** of honest inflow — the venture's hardest unsolved risk; the bot gate
  `v_i` is its veto and is itself unsolved.

> v5→v6 is the predicted "tokenomics WILL change" event, and it lands almost
> entirely in the isolated economic layer (`econ-params` + `LedgerBackend` + the
> emission rule). The product, weight math (`gem-engine`), settlement engineering,
> phasing, and 1a code carry over unchanged — v6 itself "keeps" them. Most of the
> v6 delta (demand-gated mint vs. fixed schedule, full-reserve USDC backing,
> redemption, advertiser sweep) is Phase 1b, not yet built. See ADR 0007.

## Phases (build order)

- **Phase 1a** — off-chain social product, NO real money. Gems are points. This
  is what we build first. (Core API, AI ingestion, cold-start feed, gem math
  against an off-chain ledger.)
- **Phase 1a-β** — small closed beta with capped real cash-out value.
- **Phase 1b** — real money: a **full-reserve USDC reserve**, **demand-gated
  closed-loop PEER** (mint against realized advertiser money, redeem 1:1), and an
  advertiser API + minimal attribution. On Solana, custody hot/cold + multisig.
  GATED by Gate A (legal) + Gate B (audit). Not date-committed. (v6 removes v5's
  BTC unit, floating LP, and genesis seed.)
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

## Repository layout (monorepo)

This is a **monorepo**: `backend/` (the Rust workspace — everything below) and
`frontend/` (Next.js, for the designer; consumes `backend/bindings/*.ts`). Run all
`cargo`/`docker compose` commands **from `backend/`**. Project-level docs
(`CLAUDE.md`, `ARCHITECTURE.md`, `README.md`, `docs/`) and CI live at the root.

## How to run

```sh
cd backend                     # the Rust workspace lives here
docker compose up -d           # Postgres + Redis + MinIO (object storage)
cargo test --all               # all tests (need the services running)
cargo run -p core-api          # http://localhost:8080/health
cargo run --bin transcode_worker      # async HLS transcode worker (separate process)
cargo run --bin settlement_scheduler  # auto-settles closed epochs (separate process)
```
Builds/clippy work offline via the committed `.sqlx` cache (`SQLX_OFFLINE=true`);
tests/runtime need the running services. Set `DATABASE_URL` (see `.env.example`).

## Where things are

All Rust paths below are under `backend/`.

- `crates/domain` — shared newtypes (the seam-free middle).
- `crates/econ-params` — the knobs.
- `crates/ledger` — the LedgerBackend seam + off-chain impl.
- `crates/gem-engine` — pure weight/PageRank/apportionment math.
- `crates/settlement` — epoch worker + invariants.
- `crates/core-api` — axum HTTP surface.
- `crates/storage` — S3/MinIO client (presigned upload/download).
- `migrations/` — SQL (forward-only). `0001_init.sql` includes interaction_events.
- `bindings/*.ts` — generated ts-rs frontend contract (consumed by `frontend/`).
- `../docs/adr/` + `../ARCHITECTURE.md` — decisions + the fuller map (at repo root).

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

Also built (2026-06-16): the **AI ingestion seam** (queue + operator write-back,
ADR 0006); a **settlement scheduler** binary (`settlement_scheduler`,
auto-settles closed epochs idempotently); and the **frontend type contract**
(`ts-rs` → `bindings/*.ts`).

Next steps (rough priority):
1. **AI ingestion service** (`services/ingestion`, Python) — the Rust SEAM (ADR
   0006) and now a **first Python consumer** exist: it BRPOPs `gamma:ingestion`,
   reads each post via `GET /v1/posts/:id`, and writes signals back through the
   operator-only `PUT /posts/:id/signals` into `content_signals` (JSONB). The
   analyser is a deterministic heuristic PLACEHOLDER (`model_version=heuristic-v0`,
   word/char/link counts + reading-time) — NOT a model — so the pipeline is real
   and CI-tested before the AI exists. Still to do: the real model (on the Mac
   Studio), and wiring the feed ranker to read `content_signals` (deferred until
   the signal shape is settled — no speculative ranking yet). Upgrade path: replace
   `analyzer.py`, bump `GAMMA_MODEL_VERSION`. (Tests run in CI via the
   `ingestion-python` job.)
2. **Frontend itself** (Next.js). The typed API CONTRACT now exists: `ts-rs`
   exports the public request/response types to `bindings/*.ts` (regenerated by
   `cargo test`, so CI can diff for drift). Remaining: stand up the frontend repo
   and consume `bindings/`.
3. **Multi-bitrate HLS ladder** + prod HLS delivery decision (already past 1a MVP).
4. **Smaller, tracked**: a Prometheus `/metrics` endpoint; FKs on
   `interaction_events`.

Done since the audit follow-up: MinIO+Redis+ffmpeg in CI (full media/payment path
now runs in CI); the API is versioned under `/v1` (health/ready stay unversioned);
minimal moderation (report + operator takedown/restore, soft-hide drops posts from
feed/reads); HTTP observability (tower-http TraceLayer access logs + `x-request-id`
set/propagated, correlated in the span); `time_decay_lambda` wired — interaction
edge weights are recency-decayed `e^(−λτ)` at settlement (τ = event age at epoch
close; engine stays pure; λ=0 recovers no-decay). The repo is a private GitHub
remote `meskk/gamma` (monorepo backend/ + frontend/).

Working style: deliberate, ONE reviewable step at a time; verify (tests + fmt +
clippy green) before moving on; commit each checkpoint. Tokenomics knobs are in
flux by design — keep them in `econ-params`, never hardcoded.
