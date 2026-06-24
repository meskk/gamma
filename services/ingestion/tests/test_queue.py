import json

from gamma_ingestion.queue import IngestionQueue


class FakeRedis:
    """In-memory stand-in. `redis.Redis.from_url` is lazy (no connection), so we
    build a real IngestionQueue and swap its client for this."""

    def __init__(self) -> None:
        self.items: list[bytes] = []
        self.pushed: list[tuple[str, str]] = []

    def brpop(self, keys, timeout):
        return (keys[0], self.items.pop()) if self.items else None

    def lpush(self, key, value):
        self.pushed.append((key, value))

    def close(self):
        pass


def make_queue(key: str = "gamma:ingestion") -> IngestionQueue:
    q = IngestionQueue("redis://x", key)
    q._redis = FakeRedis()  # type: ignore[attr-defined]
    return q


def test_pop_parses_an_integer_id():
    q = make_queue()
    q._redis.items = [b"42"]  # type: ignore[attr-defined]
    assert q.pop(0) == 42


def test_pop_skips_a_non_integer_payload():
    q = make_queue()
    q._redis.items = [b"not-a-number"]  # type: ignore[attr-defined]
    assert q.pop(0) is None  # robust: a junk payload doesn't crash the loop


def test_pop_returns_none_when_empty():
    assert make_queue().pop(0) is None


def test_dead_letter_pushes_json_to_the_dead_key():
    q = make_queue()
    q.dead_letter(7, "boom")
    key, value = q._redis.pushed[0]  # type: ignore[attr-defined]
    assert key == "gamma:ingestion:dead"
    assert json.loads(value) == {"post_id": 7, "error": "boom"}


def test_dead_key_derives_from_the_queue_key():
    q = make_queue(key="custom:q")
    q.dead_letter(1, "x")
    assert q._redis.pushed[0][0] == "custom:q:dead"  # type: ignore[attr-defined]
