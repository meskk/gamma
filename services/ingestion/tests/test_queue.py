import json

from gamma_ingestion.queue import IngestionQueue, _parse_post_id


class FakeRedis:
    """In-memory stand-in. `redis.Redis.from_url` is lazy (no connection), so we
    build a real IngestionQueue and swap its client for this.

    Models just enough of the LIST commands the reliable queue uses: the main list
    (``items``, tail = end), a processing list, and a dead list.
    """

    def __init__(self) -> None:
        self.items: list[bytes] = []  # main queue; brpoplpush pops the tail (end)
        self.processing: list[bytes] = []
        self.dead: list[bytes] = []
        self.pushed: list[tuple[str, str]] = []  # every lpush, for assertions

    def _as_bytes(self, value) -> bytes:
        return value if isinstance(value, bytes) else str(value).encode()

    def brpoplpush(self, src, dst, timeout):
        if not self.items:
            return None
        val = self.items.pop()  # tail of the main list
        self.processing.append(val)  # head of the processing list
        return val

    def rpoplpush(self, src, dst):
        # Recovery: move from the processing list tail back onto the main list head.
        if not self.processing:
            return None
        val = self.processing.pop()
        self.items.insert(0, val)
        return val

    def lpush(self, key, value):
        b = self._as_bytes(value)
        self.pushed.append((key, value if isinstance(value, str) else value))
        if key.endswith(":dead"):
            self.dead.insert(0, b)
        else:
            self.items.insert(0, b)

    def rpush(self, key, value):
        self.items.append(self._as_bytes(value))

    def lrem(self, key, count, value):
        b = self._as_bytes(value)
        target = self.processing if key.endswith(":processing") else self.items
        try:
            target.remove(b)
        except ValueError:
            pass

    def close(self):
        pass


def make_queue(key: str = "gamma:ingestion") -> IngestionQueue:
    q = IngestionQueue("redis://x", key)
    q._redis = FakeRedis()  # type: ignore[attr-defined]
    return q


def test_pop_parses_an_integer_id_and_moves_to_processing():
    q = make_queue()
    q._redis.items = [b"42"]  # type: ignore[attr-defined]
    result = q.pop(0)
    assert result is not None
    post_id, raw = result
    assert post_id == 42
    # The id is now in-flight on the processing list until acked.
    assert q._redis.processing == [b"42"]  # type: ignore[attr-defined]


def test_ack_removes_the_in_flight_entry():
    q = make_queue()
    q._redis.items = [b"42"]  # type: ignore[attr-defined]
    _post_id, raw = q.pop(0)  # type: ignore[misc]
    q.ack(raw)
    assert q._redis.processing == []  # type: ignore[attr-defined]


def test_pop_dead_letters_a_non_integer_payload():
    q = make_queue()
    q._redis.items = [b"not-a-number"]  # type: ignore[attr-defined]
    # Poison payload: returns None (like a timeout) but is quarantined, not dropped.
    assert q.pop(0) is None
    assert len(q._redis.dead) == 1  # type: ignore[attr-defined]
    entry = json.loads(q._redis.dead[0])  # type: ignore[attr-defined]
    assert entry["post_id"] is None
    assert "unparseable" in entry["error"]
    # And it does NOT linger on the processing list (would loop forever on recovery).
    assert q._redis.processing == []  # type: ignore[attr-defined]


def test_pop_tolerates_a_replayed_dead_letter_json_payload():
    # RPOPLPUSH <dead> <main> replays a {"post_id":…} blob; pop must normalise it.
    q = make_queue()
    q._redis.items = [json.dumps({"post_id": 99, "error": "boom"}).encode()]  # type: ignore[attr-defined]
    result = q.pop(0)
    assert result is not None
    post_id, _raw = result
    assert post_id == 99


def test_pop_returns_none_when_empty():
    assert make_queue().pop(0) is None


def test_recover_stranded_requeues_processing_ids():
    q = make_queue()
    # Simulate a crash: two ids left on the processing list, main queue empty.
    q._redis.processing = [b"7", b"8"]  # type: ignore[attr-defined]
    recovered = q.recover_stranded()
    assert sorted(recovered) == [7, 8]
    assert q._redis.processing == []  # type: ignore[attr-defined]
    # Both are back on the main queue for reprocessing.
    assert sorted(int(x) for x in q._redis.items) == [7, 8]  # type: ignore[attr-defined]


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


def test_parse_post_id_shapes():
    assert _parse_post_id(b"42") == 42
    assert _parse_post_id("42") == 42
    assert _parse_post_id(42) == 42
    assert _parse_post_id(json.dumps({"post_id": 5, "error": "e"})) == 5
    assert _parse_post_id(b"not-a-number") is None
    assert _parse_post_id(json.dumps({"error": "no id"})) is None
