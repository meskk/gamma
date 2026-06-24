"""The consume → fetch → analyse → write-back loop.

``process_post`` is the pure unit of work (no Redis, no token management) so it is
trivially testable. ``Worker`` adds token lifecycle (lazy login, re-login once on a
401) and the resilient loop: one bad post is logged and skipped, never fatal.
"""

from __future__ import annotations

import logging
from typing import Callable

from .analyzer import analyze
from .api_client import ApiClient, AuthError
from .config import Config
from .queue import IngestionQueue

log = logging.getLogger("gamma_ingestion")


def process_post(client: ApiClient, post_id: int, model_version: str, token: str) -> str:
    """Process one post id. Returns an outcome label for logging.

    ``"skipped_missing"`` if the post is gone (404), ``"written"`` on success.
    Raises ``AuthError`` (expired token) and ``ApiError`` for the caller to handle.
    """
    post = client.get_post(post_id)
    if post is None:
        return "skipped_missing"
    signals = analyze(post)
    client.put_signals(post_id, model_version, signals, token)
    return "written"


class Worker:
    def __init__(self, config: Config, queue: IngestionQueue, client: ApiClient) -> None:
        self._config = config
        self._queue = queue
        self._client = client
        self._token: str | None = None

    def _ensure_token(self) -> str:
        if self._token is None:
            self._token = self._client.login(
                self._config.operator_email, self._config.operator_password
            )
        return self._token

    def process(self, post_id: int) -> str:
        """Process one post, re-authenticating once if the token has expired."""
        token = self._ensure_token()
        try:
            return process_post(self._client, post_id, self._config.model_version, token)
        except AuthError:
            log.info("bearer token rejected; re-authenticating")
            self._token = None
            token = self._ensure_token()
            return process_post(self._client, post_id, self._config.model_version, token)

    def run(self, should_stop: Callable[[], bool]) -> None:
        """Loop until ``should_stop()`` returns True (set by a shutdown signal)."""
        self._ensure_token()
        log.info("ingestion worker started; consuming %s", self._config.queue_key)
        while not should_stop():
            post_id = self._queue.pop(self._config.poll_timeout_seconds)
            if post_id is None:
                continue  # idle timeout — re-check the stop flag and block again
            try:
                outcome = self.process(post_id)
                log.info("post %s: %s", post_id, outcome)
            except Exception:  # noqa: BLE001 — one bad post must not kill the loop
                log.exception("post %s: processing failed; skipping", post_id)
        log.info("ingestion worker stopped")
