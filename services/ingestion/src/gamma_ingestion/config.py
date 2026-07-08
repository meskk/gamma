"""Runtime configuration, read from the environment.

Every knob has a localhost-friendly default so ``gamma-ingestion`` runs against a
local ``docker compose`` stack with only the operator credentials supplied. The
operator account is how the write-back authenticates (ADR 0006 keeps the pipeline
behind the API); promote one in dev with
``UPDATE users SET role = 'operator' WHERE id = <id>;``.
"""

from __future__ import annotations

import os
from collections.abc import Mapping
from dataclasses import dataclass


class ConfigError(ValueError):
    """A required configuration value is missing or malformed."""


@dataclass(frozen=True)
class Config:
    redis_url: str
    queue_key: str
    api_base_url: str
    operator_email: str
    operator_password: str
    # How long BRPOPLPUSH blocks waiting for work before looping (seconds). Keeps the
    # process responsive to shutdown signals without busy-spinning on an empty queue.
    poll_timeout_seconds: float
    request_timeout_seconds: float
    # Which analyser the factory builds: "heuristic" (default placeholder) or
    # "model" (the real model — fails fast until the GPU/model layer exists).
    # Has a default so it can stay last in the dataclass; from_env always sets it.
    analyzer: str = "heuristic"
    # Retry policy for transient (network / 5xx) API failures. attempts == total
    # tries (1 = no retry); base_delay seeds the exponential backoff (with jitter).
    retry_attempts: int = 3
    retry_base_delay_seconds: float = 0.5
    # Where a post that still fails after retries is quarantined (defaults to
    # "<queue_key>:dead"), so failures are inspectable/replayable, not lost.
    dead_letter_key: str = "gamma:ingestion:dead"
    # Reliable-queue processing LIST: ids popped-but-not-yet-acked live here so a
    # crash mid-post can't lose them (defaults to "<queue_key>:processing").
    processing_key: str = "gamma:ingestion:processing"
    # Liveness endpoint port (/healthz, M4.1). 0 disables it (e.g. ad-hoc CLI
    # runs); the Dockerfile HEALTHCHECK and the compose probe expect the default.
    health_port: int = 8081
    # ── Model analyser (GAMMA_ANALYZER=model, M2.4c) ──────────────────────────
    # OpenAI-compatible inference endpoints on the GPU box: the judgment LLM
    # (vLLM chat completions) and the embedding encoder (TEI or a second vLLM).
    # The embed endpoint defaults to the model endpoint when unset. Inference is
    # HTTP by design: the worker needs no ML dependencies, CI stays hardware-free,
    # and each endpoint's served model id is the provenance truth (RUNBOOK §6).
    model_base_url: str = ""
    embed_base_url: str = ""
    # The topic label space the LLM classifies into — the app's category set
    # (ADR 0009: the label SPACE is the analyzer's contract). Comma-separated,
    # normalized like the app normalizes categories (trim, lowercase, dedupe).
    model_topic_labels: tuple[str, ...] = ()
    # Inference is slower than the core API; bound it separately so a stuck GPU
    # call cannot stall shutdown past the supervisor's patience.
    model_timeout_seconds: float = 60.0
    # Input budget (characters) for BOTH inference calls. Posts can legally be
    # huge (the API only caps the request at 256 KiB), but every model has a
    # context ceiling — an unbounded body would 400/413 at the server and
    # permanently dead-letter a perfectly healthy post. A judgment over a prefix
    # beats no signals; truncation is recorded in extras. Size this to the
    # smaller of the LLM's --max-model-len and the encoder's token cap.
    model_max_input_chars: int = 8000

    @staticmethod
    def from_env(environ: Mapping[str, str] | None = None) -> Config:
        env: Mapping[str, str] = os.environ if environ is None else environ

        operator_email = env.get("GAMMA_OPERATOR_EMAIL", "").strip()
        operator_password = env.get("GAMMA_OPERATOR_PASSWORD", "")
        if not operator_email or not operator_password:
            raise ConfigError(
                "GAMMA_OPERATOR_EMAIL and GAMMA_OPERATOR_PASSWORD must be set "
                "(an operator account the write-back authenticates as)."
            )

        queue_key = env.get("GAMMA_INGESTION_QUEUE", "gamma:ingestion")
        retry_attempts = _int(env, "GAMMA_RETRY_ATTEMPTS", 3)
        if retry_attempts < 1:
            raise ConfigError(f"GAMMA_RETRY_ATTEMPTS must be >= 1, got {retry_attempts}")
        health_port = _int(env, "GAMMA_HEALTH_PORT", 8081)
        if not (0 <= health_port <= 65535):
            raise ConfigError(f"GAMMA_HEALTH_PORT must be 0..65535, got {health_port}")

        analyzer = env.get("GAMMA_ANALYZER", "heuristic")
        model_base_url = env.get("GAMMA_MODEL_BASE_URL", "").strip().rstrip("/")
        embed_base_url = env.get("GAMMA_EMBED_BASE_URL", "").strip().rstrip("/") or model_base_url
        model_topic_labels = _labels(env.get("GAMMA_TOPIC_LABELS", ""))
        if analyzer == "model":
            # Fail fast at startup, not on the first post: the model analyser
            # cannot exist without its endpoints and label space.
            if not model_base_url:
                raise ConfigError(
                    "GAMMA_ANALYZER=model requires GAMMA_MODEL_BASE_URL (the "
                    "OpenAI-compatible endpoint serving the judgment LLM)."
                )
            if not model_topic_labels:
                raise ConfigError(
                    "GAMMA_ANALYZER=model requires GAMMA_TOPIC_LABELS (the app's "
                    "category set the LLM classifies topics into, comma-separated)."
                )
            # The API rejects topic labels over 64 UTF-8 bytes (invalid_topics)
            # — and would then DLQ every post the model tags with one. Catch a
            # miscopied label at startup, not one post at a time in production.
            for label in model_topic_labels:
                if len(label.encode("utf-8")) > 64:
                    raise ConfigError(
                        f"GAMMA_TOPIC_LABELS entry {label!r} exceeds 64 UTF-8 "
                        "bytes — the signals API would reject every post tagged "
                        "with it."
                    )
        return Config(
            redis_url=env.get("REDIS_URL", "redis://localhost:6379"),
            queue_key=queue_key,
            api_base_url=env.get("GAMMA_API_BASE_URL", "http://localhost:8080/v1").rstrip("/"),
            operator_email=operator_email,
            operator_password=operator_password,
            poll_timeout_seconds=_float(env, "GAMMA_POLL_TIMEOUT_SECONDS", 5.0),
            request_timeout_seconds=_float(env, "GAMMA_REQUEST_TIMEOUT_SECONDS", 10.0),
            analyzer=analyzer,
            retry_attempts=retry_attempts,
            retry_base_delay_seconds=_float(env, "GAMMA_RETRY_BASE_DELAY_SECONDS", 0.5),
            dead_letter_key=env.get("GAMMA_INGESTION_DEAD_QUEUE", f"{queue_key}:dead"),
            processing_key=env.get("GAMMA_INGESTION_PROCESSING_QUEUE", f"{queue_key}:processing"),
            health_port=health_port,
            model_base_url=model_base_url,
            embed_base_url=embed_base_url,
            model_topic_labels=model_topic_labels,
            model_timeout_seconds=_float(env, "GAMMA_MODEL_TIMEOUT_SECONDS", 60.0),
            model_max_input_chars=max(1, _int(env, "GAMMA_MODEL_MAX_CHARS", 8000)),
        )


def _labels(raw: str) -> tuple[str, ...]:
    """Normalize the topic label list the way the app normalizes categories
    (users::normalize_categories): trim, lowercase, drop empties, dedupe
    preserving first-seen order."""
    seen: dict[str, None] = {}
    for item in raw.split(","):
        label = item.strip().lower()
        if label and label not in seen:
            seen[label] = None
    return tuple(seen)


def _float(env: Mapping[str, str], key: str, default: float) -> float:
    raw = env.get(key)
    if raw is None or raw == "":
        return default
    try:
        return float(raw)
    except ValueError as exc:
        raise ConfigError(f"{key} must be a number, got {raw!r}") from exc


def _int(env: Mapping[str, str], key: str, default: int) -> int:
    raw = env.get(key)
    if raw is None or raw == "":
        return default
    try:
        return int(raw)
    except ValueError as exc:
        raise ConfigError(f"{key} must be an integer, got {raw!r}") from exc
