"""The consume → fetch → analyse → write-back loop.

``process_post`` is the pure unit of work (no Redis, no token management) so it is
trivially testable. ``Worker`` adds token lifecycle (lazy login, re-login once on a
401) and the resilient loop: one bad post is logged and skipped, never fatal. The
analyser is INJECTED (like the client and queue), so the real model swaps in without
touching this loop.
"""

from __future__ import annotations

import logging
from collections.abc import Callable

from .analyzer import Analyzer
from .api_client import ApiClient, AuthError
from .config import Config
from .metrics import Metrics
from .queue import IngestionQueue
from .retry import retry_transient

log = logging.getLogger("gamma_ingestion")

# Emit a metrics summary log every this many processed posts (plus once at shutdown).
_SUMMARY_EVERY = 100


def process_post(client: ApiClient, post_id: int, analyzer: Analyzer, token: str) -> str:
    """Process one post id. Returns an outcome label for logging.

    ``"skipped_missing"`` if the post is gone (404), ``"written"`` on success.
    The signals are stamped with ``analyzer.model_version`` — the analyser owns its
    own label, so it cannot drift from the implementation that produced it.
    Raises ``AuthError`` (expired token) and ``ApiError`` for the caller to handle.
    """
    post = client.get_post(post_id)
    if post is None:
        return "skipped_missing"
    signals = analyzer.analyze(post)
    client.put_signals(post_id, analyzer.model_version, signals, token)
    return "written"


class Worker:
    def __init__(
        self,
        config: Config,
        queue: IngestionQueue,
        client: ApiClient,
        analyzer: Analyzer,
    ) -> None:
        self._config = config
        self._queue = queue
        self._client = client
        self._analyzer = analyzer
        self._token: str | None = None
        self.metrics = Metrics()

    def _ensure_token(self) -> str:
        if self._token is None:
            self._token = self._client.login(
                self._config.operator_email, self._config.operator_password
            )
        return self._token

    def prime(self) -> None:
        """Perform the initial login up front so a bad password or unreachable API
        fails fast at startup (the caller can exit cleanly) instead of surfacing as
        an uncaught error inside the run loop. ``run`` relies on this having been
        called; ``process`` still lazily ensures a token afterwards."""
        self._ensure_token()

    def process(self, post_id: int) -> str:
        """Process one post: retry transient (network/5xx) failures with backoff,
        and re-authenticate once if the token has expired."""
        token = self._ensure_token()
        try:
            return self._run_with_retry(post_id, token)
        except AuthError:
            log.info("bearer token rejected; re-authenticating")
            self._token = None
            token = self._ensure_token()
            return self._run_with_retry(post_id, token)

    def _run_with_retry(self, post_id: int, token: str) -> str:
        return retry_transient(
            lambda: process_post(self._client, post_id, self._analyzer, token),
            attempts=self._config.retry_attempts,
            base_delay=self._config.retry_base_delay_seconds,
        )

    def run(self, should_stop: Callable[[], bool]) -> None:
        """Consume until ``should_stop()`` returns True (set by a SIGINT/SIGTERM
        handler), then shut down gracefully.

        Graceful-shutdown guarantees: the stop flag is checked only at the top of the
        loop, so a post already popped is ALWAYS finished (written or dead-lettered)
        before exit — never half-done and never lost back off the LIST — and no NEW
        post is started once stopping. Worst-case shutdown latency while idle is one
        ``poll_timeout_seconds`` (the blocking BRPOP). Per-call bounding relies on the
        API timeout (``request_timeout_seconds``); a future model analyser must
        likewise bound its own inference so an in-flight call can't stall shutdown
        (the supervisor's SIGKILL is the final backstop).

        Assumes ``prime()`` has already established the initial token (so a startup
        login failure is caught by the caller, not raised from inside this loop)."""
        log.info(
            "ingestion worker started; consuming %s with analyzer %s",
            self._config.queue_key,
            self._analyzer.model_version,
        )
        processed = 0
        while not should_stop():
            post_id = self._queue.pop(self._config.poll_timeout_seconds)
            if post_id is None:
                continue  # idle timeout — re-check the stop flag and block again
            try:
                outcome = self.process(post_id)
                self.metrics.record_outcome(outcome)
                log.info("post %s: %s", post_id, outcome)
            except AuthError:
                # A second consecutive 401 — the token is still rejected even after a
                # fresh re-login (e.g. expired/invalid operator credentials). This is a
                # credentials/environment problem, NOT a poison post. Return the id to
                # the main queue and stop so the supervisor restarts / an operator fixes
                # the credentials — do NOT dead-letter a healthy post. (A revoked role
                # yields 403, which surfaces as a plain ApiError and is dead-lettered.)
                log.error(
                    "post %s: authentication failing after re-login; returning post "
                    "and stopping",
                    post_id,
                )
                try:
                    self._queue.requeue(post_id)
                except Exception:  # noqa: BLE001 — best-effort; the id is logged above
                    log.exception("post %s: failed to requeue", post_id)
                break
            except Exception as exc:  # noqa: BLE001 — one bad post must not kill the loop
                # Quarantine, don't silently drop: the post is already off the main
                # LIST, so without this it would be lost with no record.
                log.exception("post %s: processing failed; dead-lettering", post_id)
                dead_lettered = False
                try:
                    self._queue.dead_letter(post_id, repr(exc))
                    dead_lettered = True
                except Exception:  # noqa: BLE001 — dead-letter is itself best-effort
                    log.exception("post %s: failed to dead-letter", post_id)
                self.metrics.record_failure(dead_lettered=dead_lettered)
            processed += 1
            if processed % _SUMMARY_EVERY == 0:
                log.info("ingestion metrics: %s", self.metrics.as_dict())
        log.info("ingestion worker stopped; metrics: %s", self.metrics.as_dict())
