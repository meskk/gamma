# Peer Network — frontend (Next.js)

The user-facing app. This is a **starter foundation**: it runs, and it is wired to
the backend's typed contract. The actual UX/design is built on top from here.

## Run

```sh
cd frontend
npm install
npm run dev          # http://localhost:3000
npm run typecheck    # tsc --noEmit — fails if the backend contract changed under us
```

The backend API runs separately (see `../backend`): `cd ../backend && docker compose up -d && cargo run -p core-api` → http://localhost:8080.

## The typed contract (why this is a monorepo)

The backend generates TypeScript types from its Rust request/response types
(`../backend/bindings/*.ts`, via [ts-rs](https://github.com/Aleph-Alpha/ts-rs)).
This project imports them through the `@contract/*` path alias (see
`tsconfig.json`):

```ts
import type { GemBalance } from "@contract/GemBalance";
```

Because they are the single source of truth, **a backend API shape change makes
the frontend fail to typecheck** — caught at compile time, not at runtime. The
backend regenerates the contract with `cargo test` (CI diffs it for drift).

Mapping notes: Rust `i64`/`u64` → `bigint`; `DateTime<Utc>` → `string` (ISO-8601);
server-set fields (e.g. `author_id`) are omitted from request types.

## Notes for the designer

- Stack choices beyond "Next.js + TypeScript, App Router" are open — add your
  styling (Tailwind/CSS modules/…), component library, linting, etc. as you like.
- Keep talking to `@contract/*` for anything that crosses the API boundary, so the
  compile-time safety holds.
