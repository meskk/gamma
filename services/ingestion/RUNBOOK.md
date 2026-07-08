# Ingestion service — runbook

How to deploy and operate `gamma-ingestion`, and the procedure for bringing the real
model online. Targets the chosen path: **rent a cloud GPU (Linux) now, buy later**;
the service ships as the [Docker image](./Dockerfile). (If you later run it on a
macOS box, Docker can't pass through Metal — run it natively under a launchd
LaunchDaemon instead; everything below except the container commands still applies.)

## 0. What it does

Consumes new post ids from the `gamma:ingestion` Redis queue, reads each post via the
core API, analyses it, and writes content signals back through
`PUT /v1/posts/:id/signals` (service-or-operator role). It never touches Postgres
directly (ADR 0006). Today the analyser is a deterministic heuristic placeholder; the
GPU model is a config flip away (see §6).

## 1. Build

```sh
cd services/ingestion
docker build -t gamma-ingestion:latest .
```

The image installs the exact pinned + hashed deps from `requirements.lock`, runs as a
non-root user, and starts via `python -m gamma_ingestion`.

## 2. Configure

All config is environment (see [`.env.example`](./.env.example)). Required: a
**service account** the write-back authenticates as (M2.8) — a machine identity
that may write signals but holds NONE of the human-operator powers, so a leaked
worker credential cannot settle epochs or flip bot gates. Provision it once:

```sql
-- after registering the account via POST /v1/auth/register:
UPDATE users SET role = 'service' WHERE id = <id>;
```

There is deliberately no role-escalation endpoint. (An operator account still
works for the write-back — operators can do everything a service can — but the
worker should run under 'service' everywhere beyond a dev laptop.)

Key vars: `REDIS_URL`, `GAMMA_API_BASE_URL`, `GAMMA_OPERATOR_EMAIL`,
`GAMMA_OPERATOR_PASSWORD` (the service account's credentials; the var names
predate the service role). Keep secrets out of the image — pass them at run time
(`--env-file`, a secrets manager, or the orchestrator).

**Model/data durability (so a machine move loses nothing):** the GPU box is
disposable compute. Keep model weights, LoRA adapters, and datasets in **durable EU
object storage** (versioned) — never as the only copy on the rented box. Post data and
signals live in Postgres regardless of which box runs. See the `ai-model-hardware-strategy`
note.

## 3. Run

```sh
docker run --rm --env-file .env gamma-ingestion:latest
```

The process fails fast (exit 2) on missing/invalid config or an unreachable API at
login. Restart is safe — the seam is idempotent and crash-safe — so run it under a
supervisor / `--restart=unless-stopped` / the orchestrator's restart policy.

## 4. Verify

1. Logs show `ingestion worker started; consuming gamma:ingestion with analyzer …`.
2. Create a test post via the API → the worker logs `post <id>: written`.
3. `GET /v1/admin/ingestion/status` (operator) shows `analyzed`/`unanalyzed` counts.
4. `GET /v1/posts/<id>/signals` (operator) returns the stored row.

## 5. Operate

- **Logs / metrics:** outcomes are structured logs; a metrics summary
  (`written/skipped_missing/failed/dead_lettered`) is logged every 100 posts and at
  shutdown.
- **Dead-letter queue:** posts that fail after retries land on `gamma:ingestion:dead`.
  Each entry is a JSON `{"post_id":…,"error":…}`. Inspect:
  `redis-cli LRANGE gamma:ingestion:dead 0 -1`. Replay once the cause is fixed:
  `redis-cli RPOPLPUSH gamma:ingestion:dead gamma:ingestion` per entry — the consumer
  normalises that JSON shape back to an id, so replaying does NOT destroy the entry.
  (A poison payload the consumer could not parse at all is also quarantined here, with
  `"post_id": null`; delete those rather than replaying them.)
- **Reliable delivery (at-least-once):** each id is moved onto
  `gamma:ingestion:processing` while in flight and only removed after it is written or
  dead-lettered. A crash mid-post leaves the id there; the worker re-queues any such
  stragglers at startup (logged as "recovered … stranded"). Re-delivery is safe — the
  write-back is idempotent. Inspect in-flight work: `LRANGE gamma:ingestion:processing 0 -1`.
- **Backfill the existing corpus:** `POST /v1/admin/ingestion/backfill?after=<cursor>`
  (operator), paginating until `enqueued == 0`.
- **Shutdown:** SIGINT/SIGTERM → the in-flight post finishes (never half-done) and is
  acked, then it exits (`docker stop` is clean).

## 6. The model swap (when the GPU is ready)

The model analyser EXISTS since M2.4c (`ModelAnalyzer` in `analyzer.py`): judgments
from an instruct LLM, embeddings from an encoder, both over OpenAI-compatible HTTP.
The worker carries no ML dependencies — inference is a serving concern on the GPU
box, the worker just needs URLs. Bring-up:

1. On the GPU box, serve TWO endpoints, one model each (one endpoint per model —
   provenance must be unambiguous): the judgment LLM behind vLLM (an ~8B-class
   multilingual instruct model, quantized, `--max-model-len` sized to posts) and
   the embedding encoder behind text-embeddings-inference (or a second vLLM).
   Both expose `/v1/models`, `/v1/chat/completions` / `/v1/embeddings`.
   **Sizing:** this pair fits ONE 24 GB-class GPU (L4 / A10 / RTX 4000 Ada) with
   headroom at beta throughput — the M2.5 rental target.
2. Point the worker at them: `GAMMA_MODEL_BASE_URL`, `GAMMA_EMBED_BASE_URL`
   (defaults to the model URL), `GAMMA_TOPIC_LABELS` (the app's category set —
   the label space is the analyser's contract, ADR 0009), optional
   `GAMMA_MODEL_TIMEOUT_SECONDS` (default 60) and `GAMMA_MODEL_MAX_CHARS`
   (default 8000) — the input budget for both calls; size it UNDER the smaller
   of `--max-model-len` and the encoder's token cap, so an over-long post is
   truncated by the worker (recorded in `extras`) instead of 400/413-ing at the
   server and landing in the DLQ.
3. **Stop the old worker FIRST, then start the new one — never run two analyzer
   versions against the same queue** (invariant from ADR 0009 §4). This is not
   just a race: both workers share the processing list
   (`gamma:ingestion:processing`), and worker startup runs `recover_stranded()`,
   which re-queues the OTHER worker's in-flight ids — an old worker finishing a
   retry can then overwrite a newer analysis via the blind upsert. On the VM:
   `docker compose -f compose.prod.yml stop ingestion` before the GPU-box worker
   starts. Exactly ONE worker consumes `gamma:ingestion` at any time.
4. Run with `GAMMA_ANALYZER=model`. Provenance stays no-knob: `model_version` is
   derived at startup from the model ids the endpoints THEMSELVES report via
   `/v1/models` (e.g. `llm:qwen3-8b+emb:bge-m3`) — it cannot drift from what
   actually serves, and there is still no `GAMMA_MODEL_VERSION` env knob.
   The probe runs ONCE at startup: whenever you change what an endpoint serves,
   restart the worker so the label follows (and treat that as a model swap —
   step 3 applies). Construction fails fast (clean `startup error` + exit 2) if
   an endpoint is unreachable or serves an ambiguous model list. Confirm
   `ruff check . && mypy && pytest` still green.
5. Re-analyse the corpus once a second version exists: `POST /v1/admin/ingestion/backfill`
   targeting the stale rows (version-targeted re-enqueue, prep-plan P4 — built in
   M2.6 per ADR 0009 §4; convergence is visible in `GET …/status` `by_model_version`).

The `Analyzer.analyze(post) -> dict` interface is the seam to preserve — the worker does
not change. The feed still does **not** consume signals (ADR 0006 / `feed/mod.rs`):
ADR 0009 §5 defines the consumption boundary; the wiring lands in M2.7 behind
`GAMMA_FEED_SIGNALS`.

## 7. Open (Phase 1b / pending input)

- Concrete prod `REDIS_URL` / `GAMMA_API_BASE_URL` and how the GPU box reaches prod
  (direct over TLS with Redis AUTH + an IP allowlist, or a WireGuard/Tailscale tunnel).
- ~~The service-account role replacing the shared operator credential~~ — DONE
  (M2.8): provision per §2; the worker runs under `role = 'service'`.
- ~~A liveness `/healthz` endpoint~~ — DONE (M4.1): `GET /healthz` on
  `GAMMA_HEALTH_PORT` (default 8081, 0 disables); the Dockerfile HEALTHCHECK and
  the compose healthcheck (M4.3) probe it.
- ~~The canonical signal schema (ADR 0009) — gated on the dossier.~~ DECIDED
  2026-07-08: `docs/adr/0009-versioned-signal-schema.md` (schema_version +
  typed v1 core, embeddings side table, version-targeted backfill P4,
  proposal envelope). §6 step 4 becomes real with M2.6.
