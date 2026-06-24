import pytest

from gamma_ingestion.api_client import ApiError, AuthError, TransientError
from gamma_ingestion.retry import retry_transient


def no_sleep(_delay: float) -> None:
    pass


def test_returns_on_first_success_without_sleeping():
    calls = []
    result = retry_transient(lambda: calls.append(1) or "ok", attempts=3, base_delay=0.5, sleep=no_sleep)
    assert result == "ok"
    assert len(calls) == 1


def test_retries_transient_then_succeeds_with_backoff():
    state = {"n": 0}

    def fn():
        state["n"] += 1
        if state["n"] < 3:
            raise TransientError("boom")
        return "ok"

    delays: list[float] = []
    result = retry_transient(fn, attempts=5, base_delay=1.0, sleep=delays.append, rand=lambda: 1.0)
    assert result == "ok"
    assert state["n"] == 3
    # Backoff before attempts 2 and 3: base*2^0, base*2^1 with rand()==1.0.
    assert delays == [1.0, 2.0]


def test_gives_up_after_attempts_and_reraises_last():
    def fn():
        raise TransientError("always")

    with pytest.raises(TransientError):
        retry_transient(fn, attempts=3, base_delay=0.0, sleep=no_sleep)


def test_does_not_retry_auth_error():
    calls = []

    def fn():
        calls.append(1)
        raise AuthError("401")

    with pytest.raises(AuthError):
        retry_transient(fn, attempts=3, base_delay=0.0, sleep=no_sleep)
    assert len(calls) == 1  # AuthError is handled by re-login, never retried here


def test_does_not_retry_permanent_api_error():
    calls = []

    def fn():
        calls.append(1)
        raise ApiError("400 bad request")

    with pytest.raises(ApiError):
        retry_transient(fn, attempts=3, base_delay=0.0, sleep=no_sleep)
    assert len(calls) == 1
