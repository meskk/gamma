# ADR 0001 — Backend in Rust

Status: accepted · Date: 2026-06-13

## Context

The owner prioritizes a solid, smooth, professional platform (including long
video/audio) over short-term delivery speed, and is willing to take longer and
learn Rust if it is clearly the best fit. The previous beta felt flaky and slow.
Works mostly solo, with occasional expert friends reviewing — so the codebase
must be legible to a drop-in reviewer. Tokenomics will change over time.

## Decision

Backend entirely in **Rust** (single Cargo workspace). Python for AI ingestion.
Next.js + TypeScript for the frontend (separate repo), with types generated from
the Rust API.

## Rationale

- **No GC → consistent tail latency (p99).** "Feels smooth" technically means no
  random pauses. Rust has none.
- The **money-critical core is Rust regardless** (Solana programs are Rust;
  settlement needs integer-exact, deterministic, auditable math). One language
  across the backend means one Cargo workspace, shared types, and no
  cross-language seam — fewer integration points, fewer failure modes.
- Memory safety + strong typing eliminate whole classes of runtime crashes and
  make the code self-documenting for reviewers.
- The main cost — slower iteration on product CRUD while learning — is exactly
  the price the owner has chosen to pay for long-term solidity.

## Honest caveat

Rust does NOT, by itself, fix "had to reload / slow". That is ~70% architecture +
frontend + CDN. The language choice is necessary, not sufficient. See
ARCHITECTURE.md "Media" and "stability" sections.

## Escape hatch

If product-CRUD velocity in Rust proves too painful, the **Core API** can move to
Go without touching the Rust money-core — the `LedgerBackend` trait and the
separate settlement process already isolate it. Go would be a legitimate choice,
not a shortcut. The decision is therefore low-risk and reversible.

## Consequences

- Owner invests in learning Rust; we lean on strong scaffolding + docs to help.
- CI enforces `fmt`, `clippy -D warnings`, and the invariant tests.
