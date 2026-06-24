"""Redis consumer for the ``gamma:ingestion`` queue.

The API enqueues with ``LPUSH`` (see ``core-api``'s ``IngestionQueue``); we consume
the tail with ``BRPOP`` so the pair is FIFO and the consumer blocks instead of
busy-polling an empty queue. A plain LIST, like the producer — a stream with acks
is a later durability upgrade (ADR 0006).

A post that still fails after retries is pushed to a sibling dead-letter LIST
(``<key>:dead``) instead of being silently dropped, so failures are inspectable and
replayable (see ADR 0006). Each dead-letter entry is a JSON ``{post_id, error}``.
"""

from __future__ import annotations

import json

import redis


class IngestionQueue:
    def __init__(self, redis_url: str, key: str, dead_key: str | None = None) -> None:
        self._redis = redis.Redis.from_url(redis_url)
        self._key = key
        self._dead_key = dead_key or f"{key}:dead"

    def close(self) -> None:
        self._redis.close()

    def dead_letter(self, post_id: int, error: str) -> None:
        """Quarantine a permanently-failing post for later inspection/replay."""
        self._redis.lpush(self._dead_key, json.dumps({"post_id": post_id, "error": error}))

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
