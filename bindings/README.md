# Generated TypeScript bindings

These `.ts` files are generated from the Rust API request/response types by
[ts-rs](https://github.com/Aleph-Alpha/ts-rs) — **do not edit them by hand.** They
are the typed contract between the `core-api` backend and the (separate) Next.js
frontend: a backend shape change regenerates these, so the frontend fails to
compile rather than at runtime (see `ARCHITECTURE.md`).

## Regenerate

```sh
cargo test -p core-api export_bindings
```

Generation also runs as part of `cargo test`, so CI can
`git diff --exit-code bindings/` to catch a contract drift that wasn't committed.

## Consuming from the frontend

Copy or symlink this directory into the frontend repo (e.g. `src/api-types/`), or
publish it as a package. Notes on the mapping:

- `i64` / `u64` → `bigint` (precision-safe; JS `number` loses precision past 2^53).
- timestamps (`DateTime<Utc>`) → `string` (ISO-8601).
- Server-set fields (e.g. `author_id`, derived from the session) are omitted from
  request types — the frontend never sends them.
- Internal types (auth `Principal`/`Role`, the ingestion `signals` write-back) are
  intentionally NOT exported; they are not part of the frontend surface.
