import httpx
import pytest

from gamma_ingestion.api_client import ApiClient, ApiError, AuthError, TransientError

BASE = "http://test.local/v1"


def client_with(handler) -> ApiClient:
    return ApiClient(BASE, transport=httpx.MockTransport(handler))


def test_login_returns_token():
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/v1/auth/login"
        assert request.method == "POST"
        return httpx.Response(200, json={"token": "tok-1", "user_id": 7})

    with client_with(handler) as client:
        assert client.login("op@example.com", "pw") == "tok-1"


def test_login_failure_raises():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(401, text="bad creds")

    with client_with(handler) as client:
        with pytest.raises(ApiError):
            client.login("op@example.com", "wrong")


def test_get_post_ok_and_url():
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/v1/posts/42"
        return httpx.Response(200, json={"id": 42, "author_id": 1, "body": "hi", "category": None})

    with client_with(handler) as client:
        post = client.get_post(42)
        assert post["id"] == 42


def test_get_post_404_returns_none():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(404, json={"error": "not found"})

    with client_with(handler) as client:
        assert client.get_post(999) is None


def test_put_signals_sends_bearer_and_body():
    seen = {"bodies": []}

    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path in ("/v1/posts/7/signals", "/v1/posts/8/signals")
        assert request.method == "PUT"
        seen["auth"] = request.headers.get("Authorization")
        import json

        seen["bodies"].append(json.loads(request.content))
        return httpx.Response(204)

    with client_with(handler) as client:
        client.put_signals(7, "heuristic-v1", 1, {"extras": {"word_count": 3}}, "tok-1")
        client.put_signals(
            8, "llm:x+emb:y", 1, {"quality": 0.5}, "tok-1", embedding=[0.1, 0.2]
        )

    assert seen["auth"] == "Bearer tok-1"
    assert seen["bodies"][0] == {
        "model_version": "heuristic-v1",
        "schema_version": 1,
        "signals": {"extras": {"word_count": 3}},
    }
    # With an embedding the envelope carries it NEXT TO the signals (ADR 0009 §3).
    assert seen["bodies"][1] == {
        "model_version": "llm:x+emb:y",
        "schema_version": 1,
        "signals": {"quality": 0.5},
        "embedding": [0.1, 0.2],
    }


def test_put_signals_401_raises_auth_error():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(401)

    with client_with(handler) as client:
        with pytest.raises(AuthError):
            client.put_signals(7, "heuristic-v1", 1, {}, "expired")


def test_put_signals_permanent_4xx_raises_api_error():
    # A permanent client error (e.g. unknown post → 400) is NOT retryable.
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(400, text="unknown_post")

    with client_with(handler) as client:
        with pytest.raises(ApiError) as exc:
            client.put_signals(7, "heuristic-v1", 1, {}, "tok-1")
        assert not isinstance(exc.value, TransientError)


def test_5xx_is_transient():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(503, text="busy")

    with client_with(handler) as client:
        with pytest.raises(TransientError):
            client.get_post(1)
        with pytest.raises(TransientError):
            client.put_signals(1, "m", 0, {}, "tok")


def test_transport_error_is_transient():
    def handler(_request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("no route to host")

    with client_with(handler) as client:
        with pytest.raises(TransientError):
            client.get_post(1)
