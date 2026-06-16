# ADR 0007 — Adopt the v6 economic spine (demand-gated mint, full-reserve USDC, two rails)

Status: accepted · Date: 2026-06-16 · Supersedes the v5 economic model (not the code)

## Context

The planning document advanced from v5 to **v6** (`Peer Network — Consolidated
Rebuild Dossier v6 (Pol B)`). The owner has committed to moving to v6. v6 rebuilds
**only the economic spine** and explicitly keeps v5's product, phasing, weight
math, settlement engineering, gates, and kill-path. This is precisely the
"tokenomics WILL change" event the architecture was decoupled for (ADR 0002/0003),
so it lands in the isolated economic layer rather than rippling through the
platform.

## Decision

Treat v6 as the authoritative economic model. The v5→v6 delta:

| v5 (built to) | v6 (target) |
|---|---|
| BTC unit, native custody | **USDC stablecoin** unit |
| one economy (advertiser buy-and-burn) | **two rails**: creative marketplace (stablecoin, no token) + ad-revenue token PEER |
| fixed emission (5,753/day, 21M cap, 10%/yr taper) | **demand-gated mint** `ΔPEER = (1−s_ad)·A_d`; no schedule, no cap; `A_d=0 ⇒ no mint` |
| floating BTC↔PT LP + TWAP + lockup | **full-reserve, closed-loop redemption** (`supply ≤ reserve`); no LP |
| genesis seed ~$100k | **no seed** (reserve is advertiser-funded) |
| token holdable/speculatable, DEX later | **non-tradable closed-loop redeemable credit** |

## Applicability — what carries over vs. what changes

**Carries over UNCHANGED (the bulk):**
- `gem-engine` — the social weight `w_i` (PageRank `NS`, concave `β`, log-space sum,
  Hamilton apportionment). v6 §II.3 is the v5 formula verbatim.
- Settlement engineering — integer minor-units (no floats on conserved
  quantities), Hamilton + dust→skim, idempotent epoch-keyed never-double-mint,
  fail-closed invariants, the `ledger_entries` journal (ADR 0004). v6 §II.5 ≈ what
  is already built.
- The `LedgerBackend` seam (ADR 0002) — the BTC→USDC and off-chain→Solana swap is
  an impl change behind the trait, not a rewrite.
- `econ-params` (ADR 0003) — home for the v6 dials (`s_ad`=2%, `net_mkt_fee`≈10%
  gross, etc.).
- Phase 1a product (users/posts/follows/feed, append-only interaction-graph
  capture), auth/roles, and the bot gate `v_i` (still v6's hardest unsolved risk).
- The media paid-unlock (entitlement + split routed through the ledger seam) — a
  head-start on Rail 1.

**Changes (localized to the economic spine):**
1. **Emission rule** (`settlement::emission_for`): fixed schedule → demand-gated
   mint against realized advertiser inflow. This SUPERSEDES the integer per-year
   fixed-schedule emission (commit `6899b61`) as the *rule*; the integer-exactness
   discipline, atomic `mint_epoch`, and journal all stay — only the source of the
   number changes.
2. **Backing** (a `LedgerBackend` impl, Phase 1b): full-reserve USDC, closed-loop.
   Adds a new fail-closed invariant `supply ≤ reserve` alongside the existing ones.
3. **Rail 1** becomes a first-class stablecoin marketplace (no token, no burn) —
   the unlock generalizes; its burn leg is a v5-ism that Rail 1 drops.

**New to build (almost all Phase 1b, always-deferred):** advertiser-inflow sweep +
reserve accounting (`A_d`); redemption flow (burn PEER → pay USDC, batched/rate-
limited); two-rail revenue accounting; closed-loop non-tradable semantics; the
revised Gate A (legal) / Gate B (audit).

## Consequences

- **Phase 1a needs little-to-no immediate code change** — the foundation already
  matches v6's kept spine. The big v6 changes are real-money (1b), which was never
  built. Migration is sequenced, not a rewrite.
- The fixed-schedule `emission_for` stays as the **1a points placeholder** until 1b
  (v6 keeps "Rail 1 in points first"); the demand-gated mint replaces it when real
  advertiser money exists. The exact 1a points-emission policy is an open decision.
- Honesty-floor framing replaces v5's "Theorem 8.1": conservation bounds the
  self-dealer but is silent on the open-loop bot harvest — keep that distinction
  visible (it is the central unsolved risk; bot gate `v_i` is its veto).
- All v6 numbers carry `[design]`/`[unmeasured]` markers and stay in `econ-params`,
  never hardcoded. See [[0002-ledger-backend-seam]], [[0003-economic-params-are-config]],
  [[0004-ledger-journal-and-atomic-settlement]].
