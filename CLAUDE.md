# CLAUDE.md — context for AI sessions

This file is auto-loaded by Claude Code at the start of every session. Read it
first. It exists so that any future AI session (or human) understands what this
project is and how it is built without re-deriving it from scratch.

> **Roadmap lives in [`docs/MASTERPLAN.md`](docs/MASTERPLAN.md)** — the single
> source of truth for what gets built next, in which order, with which quality
> gates, plus the append-only step ledger. Measure every session's work against
> it. This file is only updated at milestone boundaries; the MASTERPLAN carries
> the day-to-day state.

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

## Current status & next steps (snapshot 2026-07-07, M4 boundary)

The MASTERPLAN ledger (§4) is the authoritative step-by-step record; this is
the milestone-level summary. Everything listed is committed, tested, and CI-green.

Done:
- **Core product + economy (pre-plan work, audited 2026-06-16):** users, posts,
  comments, follows, cold-start feed, interaction capture; gem-engine →
  settlement → PgLedger with fail-closed conservation invariants and an
  append-only money journal; media (presigned upload, HLS transcode, paid
  unlock with creator/fee/burn split); auth (argon2 + opaque sessions), roles,
  moderation (report/takedown), `/v1` versioning, `/metrics`, structured logs.
- **M0 + M0.5:** SHA-pinned CI, blocking security scans (cargo deny, pip-audit,
  npm audit) + weekly schedule, branch protection; login hardening (per-route
  IP limits + per-account backoff in Postgres, cooldown UI, no enumeration or
  timing oracle).
- **M1 (partial):** owner decisions recorded in MASTERPLAN §5/§8 — launch
  feature matrix (P-1, hidden features behind config flags), referral system
  (P-2, BUILT: 3%/6 months, conserving cut, operator overrides), P-4 Private
  Area + P-5 Finance/YouTube-earnings model scoped (build pending decisions).
- **M2 (partial):** ingestion worker robustness cluster (P1–P3, P5–P11:
  analyzer seam, retries, DLQ, drain, metrics, CI gates; P4 — version-targeted
  re-enqueue after a model swap — is still open, the backfill only reaches
  posts with NO signals row), service-role identity (M2.8).
- **M3 (partial):** feed cursor paging (backend B1 + frontend), frontend
  unification (`useFetch`/`useLike`/`useUnlock`, admin stale-guard fix),
  German copy on all user pages.
- **M4 COMPLETE (the 1a-β ops story):** ingestion `/healthz` + backend
  Dockerfile (one image, three binaries) + `compose.prod.yml` (digest-pinned,
  no public DB ports) + `docs/OPERATIONS.md` (single-VM + Caddy TLS story);
  backup/restore (`ops/pg-backup.sh` / `ops/pg-restore.sh`, restore drilled
  twice incl. the bad-deploy schema-drift path); GHCR publish workflow
  (SHA-tagged, tag immutability enforced; deploy = `pull && up -d
  --no-build`); load smoke (`ops/load-smoke.py`, thresholds anchored to the
  10k-user model, PASS with big headroom); Go/No-Go checklist
  (`docs/GO-NO-GO-1a-beta.md`).

Next steps (per MASTERPLAN):
1. **M2.3–M2.7 — the real AI model**: ADR 0009 (versioned signal schema) →
   model analyzer behind the seam → rented EU GPU bring-up → corpus backfill →
   feed ranker reads `content_signals` behind a config flag.
2. **M3 rest:** system-account migration + deferred FKs (B2), guard tests
   (Sybil/bot-gate proptests, golden-vector snapshot BEFORE any formula
   decision, 10k–50k scale smoke).
3. **P-4/P-5** once the owner scoping + legal checks land (§5/§8).
4. **1a-β only through `docs/GO-NO-GO-1a-beta.md`** — it carries the open
   owner decisions (domain/VM provider, payout provider + cap, ADR 0010,
   verification process) and the known technical blocker: with the bundled
   MinIO, presigned URLs point at the compose-internal host, so the public
   media endpoint (managed S3 or a Caddy media subdomain) must be solved on
   the real VM first.

Working style: deliberate, ONE reviewable step at a time; verify (tests + fmt +
clippy green) before moving on; commit each checkpoint; substantive diffs get a
multi-agent adversarial review before commit, and ops artifacts are drilled for
real, not just written. Tokenomics knobs are in flux by design — keep them in
`econ-params`, never hardcoded.
