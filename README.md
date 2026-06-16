# Peer Network / Gamma

Rebuild of the Peer Network social platform (Gamma protocol) — an
ad-and-creativity-funded social platform that pays its users. Phase 1a is an
off-chain social product; the on-chain economy is gated and comes later. The
economic model targets the **v6** dossier (see `docs/adr/0007`).

This is a **monorepo**:

```
backend/     Rust workspace (the API, gem economy, settlement, media, …)
frontend/    Next.js + TypeScript app — typed against backend/bindings
docs/adr/    Architecture Decision Records
ARCHITECTURE.md, CLAUDE.md   the map + the project model & conventions
```

New here? Read **`ARCHITECTURE.md`** for the map and **`CLAUDE.md`** for the
project model and conventions.

## Backend (`backend/`)

```sh
cd backend
docker compose up -d        # Postgres + Redis + MinIO
cp .env.example .env        # if not present
cargo test --all            # all tests (need the services running)
cargo run -p core-api       # → http://localhost:8080/health
```

Requires the Rust toolchain (`rustup`, stable — pinned in
`backend/rust-toolchain.toml`) and Docker. Builds/clippy work offline via the
committed `.sqlx` cache (`SQLX_OFFLINE=true`).

## Frontend (`frontend/`)

```sh
cd frontend
npm install
npm run dev                 # → http://localhost:3000
npm run typecheck           # fails if the backend contract changed under us
```

The frontend imports the backend's generated TypeScript contract
(`backend/bindings/*.ts`, produced from the Rust types via ts-rs) through the
`@contract/*` alias — so a backend API shape change is caught at compile time.
See `frontend/README.md`.
