# Notes for a reviewer

Short, honest framing of where this repo is — so an evaluation starts from the
real state, not a guess. It is candid about gaps on purpose; the gaps below are
known and tracked, not hidden.

## What this is

A rebuild of **Peer Network** (the **Gamma** protocol): an ad-and-creativity-funded
social platform that pays its users. The authoritative plan is the **v6 dossier**;
the economic model targets the v6 spine (see `docs/adr/0007`). Monorepo:
`backend/` (Rust) + `frontend/` (Next.js).

This is **Phase 1a**: an off-chain social product where gems are points, **no real
money moves**. On-chain money (Phase 1b) is deliberately gated and not built.

## What is real and verified (Phase 1a backend)

Built, tested, and green in CI (Postgres + Redis + MinIO + ffmpeg; `cargo test`
+ fmt + clippy + a frontend-contract drift check):

- Domains (handler→service→repository): users, posts, follows, cold-start feed,
  append-only interaction-graph capture.
- Gem economy: pure weight math (`gem-engine`: PageRank + log-space + integer
  Hamilton apportionment, conservation property-tested), fail-closed settlement,
  Postgres ledger with an append-only `ledger_entries` journal, **atomic,
  crash-safe, idempotent** epoch settlement (mint-then-mark).
- Media: presigned upload/download, async HLS transcode, paid unlock routed through
  the ledger seam with the burn recorded; entitlement-gated playback.
- Auth & safety: argon2 + hashed bearer sessions; role-based operator gate;
  self-scoped reads; per-IP rate limit; body limit; interaction dedup; minimal
  moderation (report + operator takedown). API versioned under `/v1`; request-id +
  access-log tracing.

## What is deliberately deferred (and why)

These are phasing/scope decisions, documented in `CLAUDE.md` and the ADRs — not
oversights:

- **Phase 1b real-money engine** (full-reserve USDC, demand-gated mint, redemption,
  advertiser sweep) — **0% built by design**; it is gated by legal (Gate A) + audit
  (Gate B) and not date-committed. The economic layer is isolated behind
  `LedgerBackend` + `econ-params` so this lands without rippling into the platform
  (ADR 0002/0003/0007). The current 1a emission is the v5 fixed-schedule of
  valueless points.
- **Frontend** — a runnable Next.js starter typed against the backend contract
  (`backend/bindings`, ts-rs); **not yet a product**. It exists so a designer can
  build on a compile-time-checked contract.
- **AI ingestion** — the Rust seam exists (posts are queued; an operator write-back
  endpoint stores signals), but the **Python consumer does not** (runs later on
  dedicated hardware), and the feed does not consume signals yet.

## Known limitations, tracked openly

- **The bot / personhood gate `v_i` is unsolved** — and is the venture's hardest
  risk (the dossier gives it its own page). Today it is an operator-set boolean;
  the open-loop sybil-harvest problem is not solved in code, and we don't pretend
  it is. By default the economy pays nobody until an operator verifies users.
- **Weight-formula divergences from the dossier** (audience coupled to node-score;
  the `T_i` term omitted) are documented in `docs/adr/0008`, to reconcile at
  economic calibration before 1b.
- **Time-decay** `e^(−λτ)` is wired but intra-epoch only (settlement is daily), so
  its multi-day half-life can't bind yet — documented, not silent.

## On the "rigor on the easy part" critique (we agree it's the right question)

A fair read is that the heavy engineering sits on a pre-revenue ledger while the
venture-deciding unknowns (creator adoption, the bot defense, the legal gate, the
real money engine) carry the most uncertainty and the least code. We think that's
the **correct order, not misallocation**: the money-critical, hard-to-retrofit
correctness core (conservation, idempotent settlement, the decoupling seams,
irreversible interaction-graph capture) is the part you cannot safely bolt on
later, and v6 isolates the tokenomics so the unbuilt parts *can* be added behind
seams. The honest next step is **not more invariants** — it is a thin end-to-end
product into a small pilot cohort (Phase 1a-β) to actually measure the dossier's
`[unmeasured]` bets (creator volume, the sybil resolvent σ). That, not more
backend, is where the next effort should go.

## How to evaluate it yourself

- Read order: `README.md` → `ARCHITECTURE.md` → `CLAUDE.md` → `docs/adr/` (0001–0008).
- Run: `cd backend && docker compose up -d && cargo test --all` (needs Docker).
  Offline build/lint: `SQLX_OFFLINE=true cargo build/clippy` via the committed
  `.sqlx` cache.
- Git history is a clean per-checkpoint progression; each commit is green.
