# Peer Network / Gamma

Rebuild of the Peer Network social platform (Gamma protocol). Phase 1a is an
off-chain social product; the on-chain economy (Solana) is gated and comes later.

New here? Read **`ARCHITECTURE.md`** for the map and **`CLAUDE.md`** for the
project model and conventions. Decisions are in `docs/adr/`.

## Quick start

```sh
# 1. Local dependencies (Postgres + Redis)
docker compose up -d

# 2. Build & run all tests (includes settlement invariant tests)
cargo test --all

# 3. Run the Core API
cp .env.example .env
cargo run -p core-api
# → http://localhost:8080/health
```

Requires the Rust toolchain (`rustup`, stable — pinned in `rust-toolchain.toml`)
and Docker.

## Layout

```
crates/      Rust workspace (domain, econ-params, ledger, gem-engine, settlement, core-api)
migrations/  Postgres schema (forward-only)
docs/adr/    Architecture Decision Records
```
