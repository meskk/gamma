# Ingestion service — runbook

How to deploy and operate `gamma-ingestion`, and the procedure for bringing the real
model online. Targets the chosen path: **rent a cloud GPU (Linux) now, buy later**;
the service ships as the [Docker image](./Dockerfile). (If you later run it on a
macOS box, Docker can't pass through Metal — run it natively under a launchd
LaunchDaemon instead; everything below except the container commands still applies.)

## 0. What it does

Consumes new post ids from the `gamma:ingestion` Redis queue, reads each post via the
core API, analyses it, and writes content signals back through the operator-only
`PUT /v1/posts/:id/signals`. It never touches Postgres directly (ADR 0006). Today the
analyser is a deterministic heuristic placeholder; the GPU model is a config flip away
(see §6).

## 1. Build

```sh
cd services/ingestion
docker build -t gamma-ingestion:latest .
```

The image installs the exact pinned + hashed deps from `requirements.lock`, runs as a
non-root user, and starts via `python -m gamma_ingestion`.

## 2. Configure

All config is environment (see [`.env.example`](./.env.example)). Required: an
**operator account** the write-back authenticates as. Register a user via the API,
then promote it (dev): `UPDATE users SET role = 'operator' WHERE id = <id>;` — for
prod, prefer a dedicated service-account role (ADR 0005/0006 follow-up).

Key vars: `REDIS_URL`, `GAMMA_API_BASE_URL`, `GAMMA_OPERATOR_EMAIL`,
`GAMMA_OPERATOR_PASSWORD`. Keep secrets out of the image — pass them at run time
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

## 6. The model swap (when the GPU/model is ready)

1. Build/choose the model analyser: fill the single marked stub — the `model` branch of
   `make_analyzer` in `analyzer.py` (weights path, device, batch size) as an `Analyzer`
   impl that declares its own `model_version` (prep-plan P18).
2. Add the model deps to `pyproject.toml` and regenerate `requirements.lock`; base the
   Dockerfile runtime on a CUDA image (or run natively under launchd on a Mac).
3. Run with `GAMMA_ANALYZER=model`. The analyser declares its own `model_version`
   intrinsically (in code, next to the weights it loads) — there is no
   `GAMMA_MODEL_VERSION` env knob to set, so the provenance tag can't drift from the
   code. Confirm `ruff check . && mypy && pytest` still green.
4. Re-analyse the corpus once a second version exists: `POST /v1/admin/ingestion/backfill`
   targeting the stale rows (version-targeted re-enqueue, prep-plan P4).

The `Analyzer.analyze(post) -> dict` interface is the seam to preserve — the worker does
not change. The feed still does **not** consume signals (ADR 0006 / `feed/mod.rs`): wiring
ranking is a separate future ADR.

## 7. Open (Phase 1b / pending input)

- Concrete prod `REDIS_URL` / `GAMMA_API_BASE_URL` and how the GPU box reaches prod
  (direct over TLS with Redis AUTH + an IP allowlist, or a WireGuard/Tailscale tunnel).
- The service-account role replacing the shared operator credential.
- A liveness `/healthz` endpoint (prep-plan P9b) — deferred to land with the probe target
  and restart policy of the final deployment.
- The canonical signal schema (ADR 0009) — gated on the dossier.
