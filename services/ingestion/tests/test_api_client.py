import httpx
import pytest

from gamma_ingestion.api_client import ApiClient, ApiError, AuthError

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
    seen = {}

    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/v1/posts/7/signals"
        assert request.method == "PUT"
        seen["auth"] = request.headers.get("Authorization")
        import json

        seen["body"] = json.loads(request.content)
        return httpx.Response(204)

    with client_with(handler) as client:
        client.put_signals(7, "heuristic-v0", {"word_count": 3}, "tok-1")

    assert seen["auth"] == "Bearer tok-1"
    assert seen["body"] == {"model_version": "heuristic-v0", "signals": {"word_count": 3}}


def test_put_signals_401_raises_auth_error():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(401)

    with client_with(handler) as client:
        with pytest.raises(AuthError):
            client.put_signals(7, "heuristic-v0", {}, "expired")


def test_put_signals_other_error_raises_api_error():
    def handler(_request: httpx.Request) -> httpx.Response:
        return httpx.Response(500, text="boom")

    with client_with(handler) as client:
        with pytest.raises(ApiError):
            client.put_signals(7, "heuristic-v0", {}, "tok-1")
