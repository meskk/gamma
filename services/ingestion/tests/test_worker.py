from gamma_ingestion.api_client import AuthError
from gamma_ingestion.config import Config
from gamma_ingestion.worker import Worker, process_post


def make_config() -> Config:
    return Config(
        redis_url="redis://x",
        queue_key="gamma:ingestion",
        api_base_url="http://x/v1",
        operator_email="op@example.com",
        operator_password="pw",
        model_version="heuristic-v0",
        poll_timeout_seconds=0.01,
        request_timeout_seconds=1.0,
    )


class FakeClient:
    """Stand-in for ApiClient: serves posts from a dict, records write-backs, and
    can be told to reject the bearer token a fixed number of times."""

    def __init__(self, posts, fail_auth_times: int = 0):
        self.posts = posts
        self.signals_written: dict[int, dict] = {}
        self.login_count = 0
        self._fail_auth_times = fail_auth_times

    def login(self, email, password) -> str:
        self.login_count += 1
        return f"tok-{self.login_count}"

    def get_post(self, post_id):
        return self.posts.get(post_id)

    def put_signals(self, post_id, model_version, signals, token) -> None:
        if self._fail_auth_times > 0:
            self._fail_auth_times -= 1
            raise AuthError("expired")
        self.signals_written[post_id] = signals


class FakeQueue:
    def __init__(self, items):
        self._items = list(items)

    def pop(self, timeout_seconds):
        return self._items.pop(0) if self._items else None

    def drained(self) -> bool:
        return not self._items


def test_process_post_writes_signals():
    client = FakeClient({5: {"id": 5, "body": "a b c", "category": None}})
    assert process_post(client, 5, "heuristic-v0", "tok") == "written"
    assert client.signals_written[5]["word_count"] == 3


def test_process_post_skips_missing_post():
    client = FakeClient({})
    assert process_post(client, 99, "heuristic-v0", "tok") == "skipped_missing"
    assert client.signals_written == {}


def test_worker_relogins_once_on_auth_error():
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_auth_times=1)
    worker = Worker(make_config(), FakeQueue([]), client)
    assert worker.process(5) == "written"
    assert client.login_count == 2  # initial login + one re-login after the 401
    assert 5 in client.signals_written


def test_run_drains_queue_then_stops():
    client = FakeClient({5: {"id": 5, "body": "hello world", "category": None}})
    queue = FakeQueue([5])
    worker = Worker(make_config(), queue, client)
    worker.run(queue.drained)
    assert client.signals_written[5]["word_count"] == 2
    assert client.login_count == 1  # one login for the whole run
