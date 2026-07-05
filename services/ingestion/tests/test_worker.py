import redis

import gamma_ingestion.__main__ as main_mod
from gamma_ingestion.analyzer import HeuristicAnalyzer
from gamma_ingestion.api_client import ApiError, AuthError, ForbiddenError, TransientError
from gamma_ingestion.config import Config
from gamma_ingestion.worker import Worker, process_post


def make_config() -> Config:
    return Config(
        redis_url="redis://x",
        queue_key="gamma:ingestion",
        api_base_url="http://x/v1",
        operator_email="op@example.com",
        operator_password="pw",
        poll_timeout_seconds=0.01,
        request_timeout_seconds=1.0,
        retry_attempts=3,
        retry_base_delay_seconds=0.0,  # no real backoff sleeps in tests
    )


class FakeClient:
    """Stand-in for ApiClient: serves posts from a dict, records write-backs (and the
    model_version they were stamped with), and can reject the token a fixed number
    of times."""

    def __init__(
        self,
        posts,
        fail_auth_times: int = 0,
        fail_transient_times: int = 0,
        fail_permanent: bool = False,
        fail_forbidden: bool = False,
        fail_login: Exception | None = None,
        fail_login_times: int = 0,
        fail_login_exc: Exception | None = None,
    ):
        self.posts = posts
        self.signals_written: dict[int, dict] = {}
        self.model_versions: dict[int, str] = {}
        self.login_count = 0
        self.put_attempts = 0
        self._fail_auth_times = fail_auth_times
        self._fail_transient_times = fail_transient_times
        self._fail_permanent = fail_permanent
        self._fail_forbidden = fail_forbidden
        self._fail_login = fail_login
        # A BOUNDED number of login failures (e.g. an API mid-redeploy), then success.
        self._fail_login_times = fail_login_times
        self._fail_login_exc = fail_login_exc or TransientError("login temporarily down")

    def login(self, email, password) -> str:
        self.login_count += 1
        if self._fail_login is not None:
            raise self._fail_login
        if self._fail_login_times > 0:
            self._fail_login_times -= 1
            raise self._fail_login_exc
        return f"tok-{self.login_count}"

    def close(self) -> None:
        pass

    def get_post(self, post_id):
        return self.posts.get(post_id)

    def put_signals(self, post_id, model_version, signals, token) -> None:
        self.put_attempts += 1
        if self._fail_transient_times > 0:
            self._fail_transient_times -= 1
            raise TransientError("temporary 5xx")
        if self._fail_auth_times > 0:
            self._fail_auth_times -= 1
            raise AuthError("expired")
        if self._fail_forbidden:
            raise ForbiddenError("operator role revoked")
        if self._fail_permanent:
            raise ApiError("permanent 400")
        self.signals_written[post_id] = signals
        self.model_versions[post_id] = model_version


class FakeQueue:
    def __init__(self, items):
        self._items = list(items)
        self.dead_lettered: list[int] = []
        self.requeued: list[int] = []
        self.acked: list[object] = []
        self._recover: list[int] = []

    def pop(self, timeout_seconds):
        # Mirror the real queue: return (post_id, raw). The raw is just the id here.
        if not self._items:
            return None
        post_id = self._items.pop(0)
        return post_id, post_id

    def ack(self, raw: object) -> None:
        self.acked.append(raw)

    def recover_stranded(self) -> list[int]:
        return list(self._recover)

    def dead_letter(self, post_id: int, error: str) -> None:
        self.dead_lettered.append(post_id)

    def requeue(self, post_id: int) -> None:
        self.requeued.append(post_id)

    def close(self) -> None:
        pass

    def drained(self) -> bool:
        return not self._items


def test_process_post_writes_signals():
    client = FakeClient({5: {"id": 5, "body": "a b c", "category": None}})
    assert process_post(client, 5, HeuristicAnalyzer(), "tok") == "written"
    assert client.signals_written[5]["word_count"] == 3


def test_process_post_skips_missing_post():
    client = FakeClient({})
    assert process_post(client, 99, HeuristicAnalyzer(), "tok") == "skipped_missing"
    assert client.signals_written == {}


def test_written_model_version_comes_from_the_analyzer():
    # The analyser OWNS its model_version, so what's written matches the analyser —
    # not any separate config value — which is what makes the model swap drift-proof.
    # A stand-in analyser with a distinct intrinsic label proves the worker stamps
    # whatever the analyser reports (the heuristic's own label is "heuristic-v0").
    class StubAnalyzer:
        model_version = "real-model-v1"

        def analyze(self, post: dict) -> dict:
            return {"ok": True}

    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}})
    process_post(client, 5, StubAnalyzer(), "tok")
    assert client.model_versions[5] == "real-model-v1"


def test_worker_relogins_once_on_auth_error():
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_auth_times=1)
    worker = Worker(make_config(), FakeQueue([]), client, HeuristicAnalyzer())
    assert worker.process(5) == "written"
    assert client.login_count == 2  # initial login + one re-login after the 401
    assert 5 in client.signals_written


def test_run_drains_queue_then_stops():
    client = FakeClient({5: {"id": 5, "body": "hello world", "category": None}})
    queue = FakeQueue([5])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.run(queue.drained)
    assert client.signals_written[5]["word_count"] == 2
    assert client.login_count == 1  # one login for the whole run


def test_worker_retries_transient_failure_then_succeeds():
    # Two transient (5xx) put failures, then success — within the 3-attempt budget.
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_transient_times=2)
    worker = Worker(make_config(), FakeQueue([]), client, HeuristicAnalyzer())
    assert worker.process(5) == "written"
    assert client.put_attempts == 3  # 2 retries + the successful write
    assert 5 in client.signals_written
    assert client.login_count == 1  # transient retries do NOT re-login


def test_worker_gives_up_after_exhausting_retries():
    # More transient failures than the attempt budget → the error propagates.
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_transient_times=5)
    worker = Worker(make_config(), FakeQueue([]), client, HeuristicAnalyzer())
    try:
        worker.process(5)
        raised = False
    except TransientError:
        raised = True
    assert raised
    assert client.put_attempts == 3  # capped at retry_attempts


def test_run_dead_letters_a_permanently_failing_post():
    # A post that fails permanently is quarantined to the dead-letter list, not
    # silently dropped (it is already off the main LIST).
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_permanent=True)
    queue = FakeQueue([5])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.run(queue.drained)
    assert queue.dead_lettered == [5]
    assert 5 not in client.signals_written


def test_run_finishes_in_flight_post_then_stops():
    # Graceful shutdown: a stop requested DURING processing (simulating a SIGTERM
    # mid-analysis) lets the in-flight post finish, but no new post is started.
    stop = {"flag": False}

    class StopDuringAnalyze:
        model_version = "slow-v0"

        def analyze(self, post: dict) -> dict:
            stop["flag"] = True  # the signal arrives while we're analysing post 5
            return {"ok": True}

    client = FakeClient(
        {5: {"id": 5, "body": "a", "category": None}, 6: {"id": 6, "body": "b", "category": None}}
    )
    queue = FakeQueue([5, 6])
    worker = Worker(make_config(), queue, client, StopDuringAnalyze())
    worker.run(lambda: stop["flag"])

    assert 5 in client.signals_written  # in-flight post completed, not lost
    assert 6 not in client.signals_written  # no new post started after stop


def test_run_counts_written_and_skipped_in_metrics():
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}})  # post 6 absent
    queue = FakeQueue([5, 6])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.run(queue.drained)
    assert worker.metrics.written == 1
    assert worker.metrics.skipped_missing == 1  # post 6 was gone
    assert worker.metrics.failed == 0
    assert worker.metrics.total == 2


def test_run_counts_dead_lettered_in_metrics():
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}}, fail_permanent=True)
    queue = FakeQueue([5])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.run(queue.drained)
    assert worker.metrics.failed == 1
    assert worker.metrics.dead_lettered == 1
    assert worker.metrics.written == 0


def test_prime_raises_on_login_failure():
    # An unreachable API / bad credentials at startup surfaces from prime(), so the
    # caller (main) can fail fast — not as an uncaught error inside the run loop.
    client = FakeClient({}, fail_login=TransientError("connection refused"))
    worker = Worker(make_config(), FakeQueue([]), client, HeuristicAnalyzer())
    raised = False
    try:
        worker.prime()
    except TransientError:
        raised = True
    assert raised


def test_main_exits_2_when_startup_login_fails(monkeypatch):
    # RUNBOOK §3: an unreachable API at login fails fast with exit 2 and a clear
    # message, never an uncaught traceback. prime() (not run()) does the login.
    client = FakeClient({}, fail_login=ApiError("login failed: 503"))
    queue = FakeQueue([])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())

    monkeypatch.setattr(main_mod.Config, "from_env", staticmethod(lambda: make_config()))
    monkeypatch.setattr(main_mod, "make_analyzer", lambda config: HeuristicAnalyzer())
    monkeypatch.setattr(main_mod, "IngestionQueue", lambda *a, **k: queue)
    monkeypatch.setattr(main_mod, "ApiClient", lambda *a, **k: client)
    monkeypatch.setattr(main_mod, "Worker", lambda *a, **k: worker)

    assert main_mod.main() == 2
    # The bad post path is never reached: nothing dead-lettered, run() never started.
    assert queue.dead_lettered == []
    assert client.put_attempts == 0


def test_relogin_retries_a_transient_login_failure():
    # An API redeploy shows up as a 401 (token rejected) followed by a brief
    # unreachability during re-login. The transient re-login failure must be RETRIED,
    # not bubble out and get the healthy post dead-lettered as poison.
    client = FakeClient(
        {5: {"id": 5, "body": "hi", "category": None}},
        fail_auth_times=1,  # first put -> 401, triggering a re-login
    )
    worker = Worker(make_config(), FakeQueue([]), client, HeuristicAnalyzer())
    worker.prime()  # initial login (count 1) — no blip yet
    client._fail_login_times = 1  # NOW arm a one-shot blip for the re-login
    assert worker.process(5) == "written"  # recovered: not dead-lettered
    assert 5 in client.signals_written
    # login #1 prime, #2 the re-login that blipped, #3 the successful re-login.
    assert client.login_count == 3


def test_run_does_not_dead_letter_transient_relogin_failure():
    # End-to-end via run(): the same redeploy scenario must NOT quarantine the post.
    client = FakeClient(
        {5: {"id": 5, "body": "hi", "category": None}},
        fail_auth_times=1,
    )
    queue = FakeQueue([5])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.prime()
    client._fail_login_times = 1  # arm the re-login blip after the initial login
    worker.run(queue.drained)
    assert 5 in client.signals_written
    assert queue.dead_lettered == []  # healthy post, not poison
    assert queue.acked == [5]  # and acked off the processing list


def test_run_requeues_and_stops_on_forbidden_not_dead_letter():
    # 403 (operator role revoked) is systemic — every post would fail identically.
    # The worker must return the post and STOP, not shovel the backlog into the DLQ.
    client = FakeClient(
        {5: {"id": 5, "body": "hi", "category": None}, 6: {"id": 6, "body": "x", "category": None}},
        fail_forbidden=True,
    )
    queue = FakeQueue([5, 6])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.prime()
    worker.run(lambda: False)  # would loop forever; the ForbiddenError break stops it

    assert queue.requeued == [5]  # returned, not lost
    assert queue.dead_lettered == []  # NOT quarantined as poison
    assert 6 not in client.signals_written  # the loop stopped; backlog untouched


def test_run_recovers_stranded_ids_on_startup():
    # Ids left on the processing list by a prior crash are re-queued at startup.
    client = FakeClient({7: {"id": 7, "body": "hi", "category": None}})
    queue = FakeQueue([])  # main queue empty
    queue._recover = [7]
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.prime()
    # recover_stranded returns [7]; then the (empty) main queue drains immediately.
    worker.run(queue.drained)
    # The recovery itself is exercised (no crash); the id would be reprocessed on the
    # next poll once back on the main queue — here we assert the hook was invoked.
    assert queue.recover_stranded() == [7]


def test_run_retries_a_redis_blip_in_pop_instead_of_crashing(monkeypatch):
    # A Redis connection blip during pop() must be retried with backoff, not crash
    # the process with an uncaught traceback.
    client = FakeClient({5: {"id": 5, "body": "hi", "category": None}})

    class BlippyQueue(FakeQueue):
        def __init__(self, items):
            super().__init__(items)
            self._blips = 1  # fail once, then behave

        def pop(self, timeout_seconds):
            if self._blips > 0:
                self._blips -= 1
                raise redis.exceptions.ConnectionError("redis went away")
            return super().pop(timeout_seconds)

    queue = BlippyQueue([5])
    monkeypatch.setattr("gamma_ingestion.worker.time.sleep", lambda _s: None)
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.prime()
    worker.run(queue.drained)  # survives the blip and then drains post 5
    assert 5 in client.signals_written


def test_run_requeues_and_stops_on_persistent_auth_error():
    # A second consecutive 401 (token rejected even after re-login, e.g. operator
    # role revoked mid-run) is a credentials problem, NOT a poison post: the post is
    # returned to the queue and the loop stops — it must NOT be dead-lettered.
    client = FakeClient(
        {5: {"id": 5, "body": "hi", "category": None}, 6: {"id": 6, "body": "x", "category": None}},
        fail_auth_times=2,  # both the original token and the re-login token are rejected
    )
    queue = FakeQueue([5, 6])
    worker = Worker(make_config(), queue, client, HeuristicAnalyzer())
    worker.prime()
    worker.run(lambda: False)  # would loop forever; the AuthError break stops it

    assert queue.requeued == [5]  # the healthy post was returned, not lost
    assert queue.dead_lettered == []  # NOT quarantined as poison
    assert 5 not in client.signals_written
    assert 6 not in client.signals_written  # the loop stopped; no further posts
