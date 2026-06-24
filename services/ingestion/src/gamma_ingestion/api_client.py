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

    def __enter__(self) -> "ApiClient":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def login(self, email: str, password: str) -> str:
        """Authenticate and return a session bearer token."""
        resp = self._http.post(
            f"{self._base}/auth/login",
            json={"email": email, "password": password},
        )
        if resp.status_code != 200:
            raise ApiError(f"login failed: {resp.status_code} {resp.text}")
        return resp.json()["token"]

    def get_post(self, post_id: int) -> dict | None:
        """Fetch a post, or ``None`` if it no longer exists (404).

        A post can be deleted or taken down between enqueue and processing, so a
        missing post is an expected skip, not an error.
        """
        resp = self._http.get(f"{self._base}/posts/{post_id}")
        if resp.status_code == 404:
            return None
        if resp.status_code != 200:
            raise ApiError(f"get_post({post_id}) failed: {resp.status_code} {resp.text}")
        return resp.json()

    def put_signals(self, post_id: int, model_version: str, signals: dict, token: str) -> None:
        """Write back analysis for a post (operator-only). Raises on failure."""
        resp = self._http.put(
            f"{self._base}/posts/{post_id}/signals",
            json={"model_version": model_version, "signals": signals},
            headers={"Authorization": f"Bearer {token}"},
        )
        if resp.status_code == 401:
            raise AuthError(f"put_signals({post_id}) unauthorized")
        if resp.status_code != 204:
            raise ApiError(f"put_signals({post_id}) failed: {resp.status_code} {resp.text}")
