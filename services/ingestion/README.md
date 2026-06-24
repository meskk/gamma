# Gamma AI ingestion service

The Python consumer of the AI-ingestion seam (see `docs/adr/0006-ai-ingestion-seam.md`).
It closes the loop the Rust API already exposes:

```
core-api  ──LPUSH post id──►  gamma:ingestion (Redis)
                                      │  BRPOP
                                      ▼
                              gamma-ingestion
                                      │  GET /v1/posts/:id        (read content, public)
                                      │  analyse  (heuristic, deterministic)
                                      ▼
core-api  ◄──PUT /v1/posts/:id/signals── { model_version, signals }   (operator-only, 204)
                                      ▼
                              content_signals (Postgres, JSONB)
```

It **never touches Postgres directly** — every read and write goes through the API,
preserving the "API owns the database" boundary (ADR 0006 / ADR 0004).

## Status — Phase 1a placeholder

`analyzer.py` is **not a model**. It computes cheap, deterministic surface features
(word/char/link counts, a reading-time estimate, the author-declared category) so
the end-to-end pipeline is real and testable before the actual AI service exists on
the Mac Studio. The signal *shape* is deliberately minimal and the feed does **not**
consume these signals yet — wiring that in waits until the real pipeline settles the
shape, so no speculative ranking is introduced. Replacing this module and bumping
`GAMMA_MODEL_VERSION` is the upgrade path.

## Layout

```
src/gamma_ingestion/
  config.py       env-driven Config
  queue.py        Redis BRPOP consumer of gamma:ingestion
  api_client.py   login / get_post / put_signals (httpx)
  analyzer.py     deterministic heuristic signals (placeholder)
  worker.py       consume → fetch → analyse → write-back loop (+ token lifecycle)
  __main__.py     entry point (config + signal handlers + run)
tests/            unit tests (no network: httpx MockTransport + fakes)
```

## Develop

Requires Python ≥ 3.11.

```sh
cd services/ingestion
python3.12 -m venv .venv
source .venv/bin/activate
pip install -e '.[dev]'
pytest
```

## Run against a local stack

```sh
# 1. Backend + deps up (from backend/):  docker compose up -d && cargo run -p core-api
# 2. Have an operator account (register via the API, then in psql):
#      UPDATE users SET role = 'operator' WHERE id = <id>;
cp .env.example .env          # fill in GAMMA_OPERATOR_EMAIL / _PASSWORD
set -a; . ./.env; set +a
python -m gamma_ingestion
```

The worker logs one line per post (`written` / `skipped_missing`) and re-authenticates
once if its session token expires. Stop it with Ctrl-C (graceful).
