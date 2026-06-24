"""Redis consumer for the ``gamma:ingestion`` queue.

The API enqueues with ``LPUSH`` (see ``core-api``'s ``IngestionQueue``); we consume
the tail with ``BRPOP`` so the pair is FIFO and the consumer blocks instead of
busy-polling an empty queue. A plain LIST, like the producer — a stream with acks
is a later durability upgrade (ADR 0006).
"""

from __future__ import annotations

import redis


class IngestionQueue:
    def __init__(self, redis_url: str, key: str) -> None:
        self._redis = redis.Redis.from_url(redis_url)
        self._key = key

    def close(self) -> None:
        self._redis.close()

    def pop(self, timeout_seconds: float) -> int | None:
        """Block up to ``timeout_seconds`` for the next post id, or ``None`` on timeout.

        A non-integer payload is skipped (returns ``None``) rather than crashing the
        loop — the queue should only ever hold ids, but the consumer stays robust.
        """
        item = self._redis.brpop([self._key], timeout=timeout_seconds)
        if item is None:
            return None
        # brpop returns (key, value); value is bytes.
        _key, raw = item
        try:
            return int(raw)
        except (TypeError, ValueError):
            return None
