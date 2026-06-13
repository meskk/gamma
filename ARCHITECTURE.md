# Architecture — Peer Network / Gamma rebuild

This is the living build plan. It is the map a drop-in reviewer reads to
understand the system in minutes. Keep it current; when a decision changes,
update this file and add an ADR in `docs/adr/`.

## Goal

A platform that feels professional and runs smoothly — including long video and
audio — and whose foundation survives changes to the tokenomics. Correctness and
stability over short-term velocity. The previous beta felt flaky/slow; that must
not recur. Note: smoothness comes ~70% from architecture + frontend + CDN and
~30% from language choice — both are addressed below.

## Layered view

```
                 ┌──────────────────────┐
                 │  Frontend (Next.js)  │  types generated from the API
                 └───────────┬──────────┘
                             ▼
   ┌──────────────┐   ┌──────────────────┐   ┌──────────────┐
   │ Advertiser   │──►│  core-api (axum) │◄─►│  PostgreSQL  │
   │ Scan API(1b) │   └───┬──────────┬───┘   └──────────────┘
   └──────────────┘       │          ▼
                          │   ┌──────────────┐   ┌─────────────────┐
                          │   │ media service│──►│ object store+CDN│
                          │   └──────────────┘   └─────────────────┘
                          ▼
                 ┌──────────────────┐
                 │  Queue (Redis)   │
                 └───┬──────────┬───┘
                     ▼          ▼
         ┌──────────────┐  ┌────────────────────────┐
         │ AI ingestion │  │ settlement (epoch)      │
         │ (Python,Mac) │  │ gem-engine + ledger     │
         └──────────────┘  └───────────┬─────────────┘
                                       ▼
                            ┌────────────────────┐
                            │ LedgerBackend       │
                            │ 1a: OffChain        │
                            │ 1b: Solana (SPL+LP) │
                            └────────────────────┘
```

## Crate map (Rust workspace)

| Crate | Responsibility | Notes |
|-------|----------------|-------|
| `domain` | Shared newtypes: `UserId`, `Epoch`, `Sats`, `PtAmount` | No floats on money |
| `econ-params` | Every economic knob, versioned | Change tokenomics HERE |
| `ledger` | `LedgerBackend` trait + `OffChainLedger` | The phase-swap seam |
| `gem-engine` | Pure weight / PageRank / Hamilton apportionment | Deterministic, property-tested |
| `settlement` | Epoch boundary worker + invariants (fail-closed) | Idempotent, epoch-keyed |
| `core-api` | axum HTTP: users, posts, feed | `handler→service→repo` |

Added later: `services/ingestion` (Python), the Solana program (Rust/Anchor,
Phase 1b), the advertiser-scan service (Phase 1b), the frontend (separate repo).

## Media (long video/audio) — the stability-critical path

Large media NEVER streams through the API server, regardless of language:

1. Upload → object storage (S3/R2/B2), not the DB, not the app server.
2. A transcoding worker tier (ffmpeg) produces multiple quality renditions.
3. Long video/audio served as **HLS/DASH adaptive streams** over a **CDN** with
   byte-range requests + signed URLs.
4. The API only hands out metadata + URLs.

This is what makes "professional, no buffering" true. It is the cleanest service
to extract first (strangler-fig; the media blobs are immutable and stateless).

## Build sequence

0. **Phase 0 (now):** repo + CI + Git workflow (done in this scaffold); lock
   feed/settlement schemas; lock the frontend API contract.
1. Core API skeleton + auth + Postgres + **interaction-graph capture in week one**
   (cannot be backfilled).
2. Media service (object storage + CDN + transcoding).
3. AI ingestion + cold-start feed.
4. Gem-engine + off-chain settlement worker (done as a runnable scaffold here).
5. Frontend integration (external design, in-house fallback).
6. Phase 1a-β capped value.
7. Phase 1b (gated): Solana program → LP/custody → advertiser API. Economic spine
   (buy-and-burn → escrow Q → settlement → LP) sequenced first.

## Invariants the settlement worker enforces (fail-closed)

From Dossier Appendix B.2. Implemented as code, asserted every epoch:
(i) conservation Σ payouts == emission; (ii) emission independence (never a
function of burns/demand); (iii) supply monotonicity; (iv) escrow conservation
(1b); (v) company skim exactly 2% from existing tokens (1b); (vi) constant-product
LP integrity (1b). The scaffold implements (i)–(iii); (iv)–(vi) land with the
Solana backing.

## Portability & multi-machine

Everything needed to build is in the repo: pinned toolchain
(`rust-toolchain.toml`), `docker-compose.yml` for deps, `.env.example`. Clone +
`docker compose up -d` + `cargo test --all` on any machine. Push to a Git remote
so moving to a new computer is a clone, not a migration.
