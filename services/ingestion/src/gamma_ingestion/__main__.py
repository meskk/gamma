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
from .queue import IngestionQueue
from .worker import Worker


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )

    try:
        config = Config.from_env()
        # NotImplementedError: GAMMA_ANALYZER=model set before the model exists.
        analyzer = make_analyzer(config)
    except (ConfigError, NotImplementedError) as exc:
        print(f"startup error: {exc}", file=sys.stderr)
        return 2

    stop = threading.Event()
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    signal.signal(signal.SIGTERM, lambda *_: stop.set())

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
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
