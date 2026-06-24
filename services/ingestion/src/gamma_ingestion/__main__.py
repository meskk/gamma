"""Entry point: wire config + signals + worker, then run until interrupted.

Run with ``python -m gamma_ingestion`` or the ``gamma-ingestion`` console script.
"""

from __future__ import annotations

import logging
import signal
import sys
import threading

from .analyzer import HeuristicAnalyzer
from .api_client import ApiClient
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
    except ConfigError as exc:
        print(f"configuration error: {exc}", file=sys.stderr)
        return 2

    stop = threading.Event()
    signal.signal(signal.SIGINT, lambda *_: stop.set())
    signal.signal(signal.SIGTERM, lambda *_: stop.set())

    queue = IngestionQueue(config.redis_url, config.queue_key)
    client = ApiClient(config.api_base_url, config.request_timeout_seconds)
    # The heuristic placeholder for now; P2 replaces this line with a config-driven
    # factory (GAMMA_ANALYZER=heuristic|model) that selects the real model.
    analyzer = HeuristicAnalyzer(model_version=config.model_version)
    worker = Worker(config, queue, client, analyzer)
    try:
        worker.run(stop.is_set)
    finally:
        client.close()
        queue.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
