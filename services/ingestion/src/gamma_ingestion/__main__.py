"""Entry point: wire config + signals + worker, then run until interrupted.

Run with ``python -m gamma_ingestion`` or the ``gamma-ingestion`` console script.
"""

from __future__ import annotations

import logging
import signal
import sys
import threading

from .analyzer import make_analyzer
from .api_client import ApiClient, ApiError, AuthError, TransientError
from .config import Config, ConfigError
from .health import start_health_server
from .queue import IngestionQueue
from .worker import Worker


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    try:
        config = Config.from_env()
        # The model analyser probes its inference endpoints at construction
        # (fail fast, RUNBOOK §6): an unreachable GPU box surfaces here as a
        # TransientError/ApiError, not as a traceback crash-loop.
        analyzer = make_analyzer(config)
    except (ConfigError, ApiError) as exc:
        print(f"startup error: {exc}", file=sys.stderr)
        return 2

    stop = threading.Event()
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    signal.signal(signal.SIGTERM, lambda *_: stop.set())

    # Liveness probe (M4.1): a daemon thread, so it never blocks shutdown.
    health = start_health_server(config.health_port) if config.health_port else None

    queue = IngestionQueue(
        config.redis_url,
        config.queue_key,
        config.dead_letter_key,
        config.processing_key,
    )
    client = ApiClient(config.api_base_url, config.request_timeout_seconds)
    worker = Worker(config, queue, client, analyzer)
    try:
        # Log in up front so a bad password / unreachable API fails fast (exit 2 with a
        # clear message — RUNBOOK §3), not as an uncaught traceback inside the loop.
        try:
            worker.prime()
        except (ApiError, AuthError, TransientError) as exc:
            print(f"startup error: {exc}", file=sys.stderr)
            return 2
        worker.run(stop.is_set)
    finally:
        client.close()
        queue.close()
        if health is not None:
            health.shutdown()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
