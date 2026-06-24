"""Runtime configuration, read from the environment.

Every knob has a localhost-friendly default so ``gamma-ingestion`` runs against a
local ``docker compose`` stack with only the operator credentials supplied. The
operator account is how the write-back authenticates (ADR 0006 keeps the pipeline
behind the API); promote one in dev with
``UPDATE users SET role = 'operator' WHERE id = <id>;``.
"""

from __future__ import annotations

import os
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
    model_version: str
    # How long BRPOP blocks waiting for work before looping (seconds). Keeps the
    # process responsive to shutdown signals without busy-spinning on an empty queue.
    poll_timeout_seconds: float
    request_timeout_seconds: float
    # Which analyser the factory builds: "heuristic" (default placeholder) or
    # "model" (the real model — fails fast until the GPU/model layer exists).
    # Has a default so it can stay last in the dataclass; from_env always sets it.
    analyzer: str = "heuristic"

    @staticmethod
    def from_env(environ: dict[str, str] | None = None) -> "Config":
        env = os.environ if environ is None else environ

        operator_email = env.get("GAMMA_OPERATOR_EMAIL", "").strip()
        operator_password = env.get("GAMMA_OPERATOR_PASSWORD", "")
        if not operator_email or not operator_password:
            raise ConfigError(
                "GAMMA_OPERATOR_EMAIL and GAMMA_OPERATOR_PASSWORD must be set "
                "(an operator account the write-back authenticates as)."
            )

        return Config(
            redis_url=env.get("REDIS_URL", "redis://localhost:6379"),
            queue_key=env.get("GAMMA_INGESTION_QUEUE", "gamma:ingestion"),
            api_base_url=env.get("GAMMA_API_BASE_URL", "http://localhost:8080/v1").rstrip("/"),
            operator_email=operator_email,
            operator_password=operator_password,
            model_version=env.get("GAMMA_MODEL_VERSION", "heuristic-v0"),
            poll_timeout_seconds=_float(env, "GAMMA_POLL_TIMEOUT_SECONDS", 5.0),
            request_timeout_seconds=_float(env, "GAMMA_REQUEST_TIMEOUT_SECONDS", 10.0),
            analyzer=env.get("GAMMA_ANALYZER", "heuristic"),
        )


def _float(env: dict[str, str], key: str, default: float) -> float:
    raw = env.get(key)
    if raw is None or raw == "":
        return default
    try:
        return float(raw)
    except ValueError as exc:
        raise ConfigError(f"{key} must be a number, got {raw!r}") from exc
