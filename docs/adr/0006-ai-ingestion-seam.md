# ADR 0006 — AI ingestion seam (queue in, signals write-back out)

Status: accepted · Date: 2026-06-16

## Context

Phase 1a calls for an AI ingestion service (Python, on a Mac Studio) that analyses
content and feeds signals into the cold-start feed. That service and its hardware
do not exist yet, but its boundary with the Rust API can be defined now so it
plugs in later with zero changes above the line — the same "seam first" approach
used for the ledger and the transcode worker.

## Decision

Define the seam in two directions; build only the Rust side now (no Python, no AI
logic, no feed-ranking change):

- **In (offer content):** a Redis LIST queue `gamma:ingestion`
  (`queue::IngestionQueue`). `PostService::create` enqueues each new post id,
  best-effort — a Redis failure logs but never fails the post (the post is the
  source of truth; the pipeline can also backfill). Mirrors media finalize →
  transcode enqueue.
- **Out (write results back):** the service stores its analysis via an
  operator-authenticated endpoint, `PUT /posts/:id/signals`
  `{ model_version, signals }`, which upserts the `content_signals` table
  (migration 0011). The service NEVER touches Postgres directly — all writes stay
  behind the API (the discipline ADR 0004 restored for the unlock path).

`content_signals.signals` is JSONB: the pipeline owns its output shape (topic,
quality, later embeddings, …) and can evolve it without a Rust migration.

## Rationale

A one-directional queue nothing reads back from isn't a usable contract; defining
both directions now lets the Python service be written against a precise spec.
Routing the write-back through an authed API endpoint (not direct DB access)
keeps the "API owns the database" boundary intact and avoids a second writer the
schema doesn't control. JSONB keeps the signal shape unset until the pipeline
exists, so we don't lock in the wrong columns prematurely.

## Consequences

- The feed does NOT consume signals yet — that waits until the pipeline's output
  shape is known, so no speculative ranking weights are introduced. Rows simply
  don't exist until the service runs, and the feed falls back to its deterministic
  ranking. Wiring the feed to read `content_signals` is the next step once the
  Python side lands.
- The write-back is operator-only for now; a dedicated service-account role can
  replace the shared operator credential later. See [[0005-bot-gate-is-operator-only]].
- The ingestion queue, like the transcode queue, is a plain Redis LIST; a stream
  with acks is a later durability upgrade.
- The Python consumer adds pragmatic durability short of that upgrade: transient
  failures (network / 5xx) are retried with exponential backoff + jitter, and a post
  that still fails — or fails permanently (a 4xx) — is pushed to a sibling
  **dead-letter LIST** `<key>:dead` (default `gamma:ingestion:dead`) as a JSON
  `{post_id, error}`, rather than being silently dropped off the main LIST. Inspect
  with `LRANGE`; replay by pushing the ids back onto `gamma:ingestion`.
