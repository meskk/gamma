"""Thin client for the core API.

The ingestion service reads posts and writes signals ONLY through the API — never
straight to Postgres (ADR 0006). Reads are public; the write-back is operator-only
and carries a bearer token from ``/auth/login``.

URLs are built by explicit concatenation against ``base_url`` (which already
includes the ``/v1`` prefix) rather than httpx's ``base_url`` join, which discards
the base path for absolute-path references — a common, silent footgun.
"""

from __future__ import annotations

import httpx


class ApiError(RuntimeError):
    """An API call returned an unexpected status."""


class AuthError(ApiError):
    """The bearer token was rejected (401) — it likely expired; re-login."""


class ForbiddenError(ApiError):
    """The credentials are valid but lack permission (403) — e.g. the operator role
    was revoked. This is a SYSTEMIC failure of the whole worker, not a poison post:
    re-login won't help and every post would fail identically, so the worker must
    stop rather than shovel the entire backlog into the dead-letter queue."""


class TransientError(ApiError):
    """A retryable failure: a network/transport error or a 5xx server response.
    The real (slower) model widens the window for these, so they are retried with
    backoff before a post is given up on. Distinct from permanent 4xx errors."""


class ApiClient:
    """Synchronous core-API client. One per worker; not thread-safe.

    ``transport`` is injectable so tests can drive it with ``httpx.MockTransport``
    instead of a live server.
    """

    def __init__(
        self,
        base_url: str,
        timeout_seconds: float = 10.0,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        self._base = base_url.rstrip("/")
        self._http = httpx.Client(timeout=timeout_seconds, transport=transport)

    def close(self) -> None:
        self._http.close()

    def __enter__(self) -> ApiClient:
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def _send(self, method: str, url: str, **kwargs: object) -> httpx.Response:
        """Send a request, turning httpx transport failures (timeouts, connection
        resets) into a retryable ``TransientError``."""
        try:
            return self._http.request(method, url, **kwargs)  # type: ignore[arg-type]
        except httpx.TransportError as exc:
            raise TransientError(f"{method} {url}: transport error: {exc}") from exc

    def login(self, email: str, password: str) -> str:
        """Authenticate and return a session bearer token."""
        resp = self._send(
            "POST",
            f"{self._base}/auth/login",
            json={"email": email, "password": password},
        )
        if resp.status_code >= 500:
            raise TransientError(f"login failed: {resp.status_code}")
        if resp.status_code != 200:
            raise ApiError(f"login failed: {resp.status_code} {resp.text}")
        token: str = resp.json()["token"]
        return token

    def get_post(self, post_id: int) -> dict | None:
        """Fetch a post, or ``None`` if it no longer exists (404).

        A post can be deleted or taken down between enqueue and processing, so a
        missing post is an expected skip, not an error.
        """
        resp = self._send("GET", f"{self._base}/posts/{post_id}")
        if resp.status_code == 404:
            return None
        if resp.status_code >= 500:
            raise TransientError(f"get_post({post_id}) failed: {resp.status_code}")
        if resp.status_code != 200:
            raise ApiError(f"get_post({post_id}) failed: {resp.status_code} {resp.text}")
        post: dict = resp.json()
        return post

    def put_signals(self, post_id: int, model_version: str, signals: dict, token: str) -> None:
        """Write back analysis for a post (operator-only). Raises on failure."""
        resp = self._send(
            "PUT",
            f"{self._base}/posts/{post_id}/signals",
            json={"model_version": model_version, "signals": signals},
            headers={"Authorization": f"Bearer {token}"},
        )
        if resp.status_code == 401:
            raise AuthError(f"put_signals({post_id}) unauthorized")
        if resp.status_code == 403:
            # Valid token, insufficient permission (operator role revoked): systemic,
            # not per-post — surface it so the worker stops instead of dead-lettering.
            raise ForbiddenError(f"put_signals({post_id}) forbidden")
        if resp.status_code >= 500:
            raise TransientError(f"put_signals({post_id}) failed: {resp.status_code}")
        if resp.status_code != 204:
            raise ApiError(f"put_signals({post_id}) failed: {resp.status_code} {resp.text}")
