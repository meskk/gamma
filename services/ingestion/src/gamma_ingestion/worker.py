"""The consume → fetch → analyse → write-back loop.

``process_post`` is the pure unit of work (no Redis, no token management) so it is
trivially testable. ``Worker`` adds token lifecycle (lazy login, re-login once on a
401) and the resilient loop: one bad post is logged and skipped, never fatal. The
analyser is INJECTED (like the client and queue), so the real model swaps in without
touching this loop.

Delivery is at-least-once (see ``queue.py``): the queue moves each id onto a
processing list at ``pop`` and only drops it at ``ack`` (after the post is written or
dead-lettered), so a crash mid-post re-delivers rather than loses it. The write-back
is idempotent, so a re-delivery is harmless.
"""

from __future__ import annotations

import logging
import time
from collections.abc import Callable

import redis

from .analyzer import EMBEDDING_KEY, Analyzer
from .api_client import ApiClient, AuthError, ForbiddenError
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
    The signals are stamped with ``analyzer.model_version`` and the analyser's
    ``schema_version`` (ADR 0009) — the analyser owns both, so neither the
    provenance label nor the contract version can drift from the implementation
    that produced them.
    Raises ``AuthError`` (expired token) and ``ApiError`` for the caller to handle.
    """
    post = client.get_post(post_id)
    if post is None:
        return "skipped_missing"
    signals = analyzer.analyze(post)
    # ADR 0009 §3: the embedding rides the write-back ENVELOPE, never the signals
    # document — lift the reserved key out before the wire. (If this strip were
    # ever forgotten, the API would reject the write with unknown_signal_field —
    # fail closed, not silently stored.)
    embedding = signals.pop(EMBEDDING_KEY, None)
    client.put_signals(
        post_id,
        analyzer.model_version,
        analyzer.schema_version,
        signals,
        token,
        embedding=embedding,
    )
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
        and re-authenticate once if the token has expired.

        The re-login on a 401 is itself wrapped in the transient-retry: an API
        redeploy shows up as a 401 followed by a brief unreachability, and a
        transient failure while re-logging in must NOT bubble out to the caller as a
        poison-post error — the post is healthy and retryable."""
        token = self._ensure_token()
        try:
            return self._run_with_retry(post_id, token)
        except AuthError:
            log.info("bearer token rejected; re-authenticating")
            self._token = None
            token = self._relogin_with_retry()
            return self._run_with_retry(post_id, token)

    def _run_with_retry(self, post_id: int, token: str) -> str:
        return retry_transient(
            lambda: process_post(self._client, post_id, self._analyzer, token),
            attempts=self._config.retry_attempts,
            base_delay=self._config.retry_base_delay_seconds,
        )

    def _relogin_with_retry(self) -> str:
        """Re-login, retrying transient failures with backoff.

        ``login`` raises ``TransientError`` on network/5xx problems (an API mid-
        redeploy). Retrying here — instead of letting it propagate — keeps a brief
        blip during re-auth from quarantining an in-flight, healthy post as poison."""
        return retry_transient(
            self._ensure_token,
            attempts=self._config.retry_attempts,
            base_delay=self._config.retry_base_delay_seconds,
        )

    def _pop_with_retry(self, should_stop: Callable[[], bool]) -> tuple[int, object] | None:
        """Pop the next id, retrying Redis connection blips with backoff.

        A transient Redis failure (a restart / network blip) must not kill the
        process with an uncaught traceback: retry with exponential backoff until the
        queue comes back or shutdown is requested. Returns ``None`` on an idle
        timeout or when stopping."""
        delay = self._config.retry_base_delay_seconds or 0.5
        while not should_stop():
            try:
                return self._queue.pop(self._config.poll_timeout_seconds)
            except redis.exceptions.RedisError:
                log.warning(
                    "redis unavailable while polling; retrying in %.2fs", delay, exc_info=True
                )
                time.sleep(delay)
                delay = min(delay * 2, 30.0)
        return None

    def run(self, should_stop: Callable[[], bool]) -> None:
        """Consume until ``should_stop()`` returns True (set by a SIGINT/SIGTERM
        handler), then shut down gracefully.

        Graceful-shutdown guarantees: the stop flag is checked only at the top of the
        loop, so a post already popped is ALWAYS finished (written or dead-lettered)
        and acked before exit — never half-done and never lost off the processing
        LIST — and no NEW post is started once stopping. Worst-case shutdown latency
        while idle is one ``poll_timeout_seconds`` (the blocking BRPOPLPUSH). Per-call
        bounding relies on the API timeout (``request_timeout_seconds``); a future
        model analyser must likewise bound its own inference so an in-flight call
        can't stall shutdown (the supervisor's SIGKILL is the final backstop).

        Assumes ``prime()`` has already established the initial token (so a startup
        login failure is caught by the caller, not raised from inside this loop)."""
        stranded = self._queue.recover_stranded()
        if stranded:
            log.warning(
                "recovered %d id(s) stranded on the processing list from a prior "
                "crash; re-queued for reprocessing: %s",
                len(stranded),
                stranded,
            )
        log.info(
            "ingestion worker started; consuming %s with analyzer %s",
            self._config.queue_key,
            self._analyzer.model_version,
        )
        processed = 0
        while not should_stop():
            popped = self._pop_with_retry(should_stop)
            if popped is None:
                continue  # idle timeout / stopping — re-check the stop flag
            post_id, raw = popped
            try:
                outcome = self.process(post_id)
                self.metrics.record_outcome(outcome)
                log.info("post %s: %s", post_id, outcome)
            except AuthError:
                # A second consecutive 401 — the token is still rejected even after a
                # fresh re-login (e.g. expired/invalid operator credentials). This is a
                # credentials/environment problem, NOT a poison post. Return the id to
                # the main queue and stop so the supervisor restarts / an operator fixes
                # the credentials — do NOT dead-letter a healthy post.
                log.error(
                    "post %s: authentication failing after re-login; returning "
                    "post and stopping",
                    post_id,
                )
                self._safe_requeue(post_id)
                self._safe_ack(raw)  # the id is now back on the main queue; drop the in-flight copy
                return
            except ForbiddenError:
                # 403: valid token, insufficient permission (operator role revoked).
                # Systemic, not per-post — every post would fail identically, so
                # return this one and stop rather than shovel the whole backlog into
                # the DLQ.
                log.error(
                    "post %s: forbidden (operator permission revoked?); returning "
                    "post and stopping",
                    post_id,
                )
                self._safe_requeue(post_id)
                self._safe_ack(raw)  # the id is now back on the main queue; drop the in-flight copy
                return
            except Exception as exc:  # noqa: BLE001 — one bad post must not kill the loop
                # Quarantine, don't silently drop.
                log.exception("post %s: processing failed; dead-lettering", post_id)
                dead_lettered = False
                try:
                    self._queue.dead_letter(post_id, repr(exc))
                    dead_lettered = True
                except Exception:  # noqa: BLE001 — dead-letter is itself best-effort
                    log.exception("post %s: failed to dead-letter", post_id)
                self.metrics.record_failure(dead_lettered=dead_lettered)
            # The post is fully handled (written, skipped, or dead-lettered) — ack it
            # off the processing list so a later crash won't re-deliver it.
            self._safe_ack(raw)
            processed += 1
            if processed % _SUMMARY_EVERY == 0:
                log.info("ingestion metrics: %s", self.metrics.as_dict())
        log.info("ingestion worker stopped; metrics: %s", self.metrics.as_dict())

    def _safe_requeue(self, post_id: int) -> None:
        try:
            self._queue.requeue(post_id)
        except Exception:  # noqa: BLE001 — best-effort; recovered from the processing list otherwise
            log.exception("post %s: failed to requeue", post_id)

    def _safe_ack(self, raw: object) -> None:
        try:
            self._queue.ack(raw)
        except Exception:  # noqa: BLE001 — a failed ack only risks a harmless re-delivery
            log.exception("failed to ack processed id off the processing list")
