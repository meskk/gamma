"""Redis consumer for the ``gamma:ingestion`` queue.

The API enqueues with ``LPUSH`` (see ``core-api``'s ``IngestionQueue``); we consume
the tail with a **reliable-queue** pattern (``BRPOPLPUSH``) so the pair is FIFO and
the consumer blocks instead of busy-polling an empty queue.

Reliable delivery (at-least-once): ``pop`` atomically moves an id from the main
LIST onto a per-worker *processing* LIST and returns it; the id stays on the
processing LIST until the worker calls ``ack`` (after the post is written or
dead-lettered). If the process is SIGKILLed/OOMs between the pop and the ack, the
id survives on the processing LIST and is recovered by ``recover_stranded`` at the
next startup. This is at-least-once, not exactly-once: a crash after the write but
before the ack re-delivers the post, and the write-back (``PUT …/signals``) is
idempotent (upsert), so a re-delivery is harmless.

A post that still fails after retries is pushed to a sibling dead-letter LIST
(``<key>:dead``) instead of being silently dropped, so failures are inspectable and
replayable (see ADR 0006). Each dead-letter entry is a JSON ``{post_id, error}``.
Replaying an entry back onto the main queue (``RPOPLPUSH <key>:dead <key>``) works
because ``pop`` normalises both a bare int and that JSON shape to a post id.
"""

from __future__ import annotations

import json
from typing import cast

import redis


def _parse_post_id(raw: object) -> int | None:
    """Normalise a queue payload to a post id, tolerating both shapes we may see.

    The main queue holds bare integer ids (the producer LPUSHes ``post_id``), but a
    replayed dead-letter entry is the JSON ``{"post_id": …, "error": …}`` this module
    writes — supporting both lets ``RPOPLPUSH <dead> <main>`` replay work without
    losing the id. Returns ``None`` for anything that is neither (a poison payload).
    """
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", "replace")
    if isinstance(raw, int):
        return raw
    if not isinstance(raw, str):
        return None
    text = raw.strip()
    try:
        return int(text)
    except (TypeError, ValueError):
        pass
    # Not a bare int — maybe a replayed dead-letter JSON blob.
    try:
        obj = json.loads(text)
    except (TypeError, ValueError):
        return None
    if isinstance(obj, dict):
        post_id = obj.get("post_id")
        if isinstance(post_id, int):
            return post_id
    return None


class IngestionQueue:
    def __init__(
        self,
        redis_url: str,
        key: str,
        dead_key: str | None = None,
        processing_key: str | None = None,
    ) -> None:
        self._redis = redis.Redis.from_url(redis_url)
        self._key = key
        self._dead_key = dead_key or f"{key}:dead"
        # A per-key processing LIST holds ids that have been popped but not yet acked,
        # so a crash mid-processing can't lose them (they are recovered at startup).
        self._processing_key = processing_key or f"{key}:processing"

    def close(self) -> None:
        self._redis.close()

    def dead_letter(self, post_id: int, error: str) -> None:
        """Quarantine a permanently-failing post for later inspection/replay."""
        self._redis.lpush(self._dead_key, json.dumps({"post_id": post_id, "error": error}))

    def requeue(self, post_id: int) -> None:
        """Return a popped id to the main queue for reprocessing.

        ``pop`` consumes the tail (the producer LPUSHes onto the head), so we RPUSH
        here — the requeued id goes back on the tail and the very next ``pop`` picks
        it up promptly, ahead of the existing backlog rather than lost. Used when a
        transient/credentials problem (not the post) caused the failure.
        """
        self._redis.rpush(self._key, post_id)

    def ack(self, raw: object) -> None:
        """Remove a processed id from the processing LIST.

        ``raw`` is the exact bytes ``pop`` moved onto the processing LIST (returned
        alongside the parsed id), so ``LREM`` removes precisely that entry — not a
        different in-flight copy of the same id.
        """
        # redis-py's lrem stub types the value as str, but redis encodes bytes too and
        # `raw` is the exact bytes from brpoplpush — cast so LREM matches byte-for-byte.
        self._redis.lrem(self._processing_key, 1, cast(str, raw))

    def recover_stranded(self) -> list[int]:
        """Return ids left on the processing LIST by a previous crashed run.

        Called once at startup: any id still on the processing LIST was popped but
        never acked (the worker died between pop and ack), so it is moved back onto
        the main queue's tail for reprocessing. Returns the recovered ids (for
        logging). Idempotent — a clean shutdown leaves the list empty.
        """
        recovered: list[int] = []
        while True:
            raw = self._redis.rpoplpush(self._processing_key, self._key)
            if raw is None:
                break
            post_id = _parse_post_id(raw)
            if post_id is not None:
                recovered.append(post_id)
        return recovered

    def pop(self, timeout_seconds: float) -> tuple[int, object] | None:
        """Block up to ``timeout_seconds`` for the next post id, or ``None`` on timeout.

        Atomically moves the id from the main LIST onto the per-worker processing
        LIST (``BRPOPLPUSH``) so a crash before ``ack`` cannot lose it. Returns the
        parsed ``post_id`` together with the RAW payload (pass the raw back to ``ack``
        so the exact entry is removed). A poison payload (neither a bare int nor a
        ``{post_id}`` JSON blob) is dead-lettered and skipped (returns ``None``, as if
        the poll timed out) rather than being silently dropped or crashing the loop.
        """
        raw = self._redis.brpoplpush(self._key, self._processing_key, timeout=timeout_seconds)
        if raw is None:
            return None
        post_id = _parse_post_id(raw)
        if post_id is None:
            # Poison payload: quarantine it (not a valid id) and drop it off the
            # processing LIST so it doesn't get recovered into an infinite loop.
            self._redis.lpush(
                self._dead_key,
                json.dumps({"post_id": None, "error": f"unparseable payload: {raw!r}"}),
            )
            self._redis.lrem(self._processing_key, 1, cast(str, raw))
            return None
        return post_id, raw
