"""Bounded retry for transient failures.

Wraps a call and retries it on ``TransientError`` (network/5xx) with exponential
backoff and full jitter, up to a fixed number of attempts. Anything else —
``AuthError`` (handled by re-login), permanent 4xx ``ApiError``, a deleted post —
propagates immediately and is NOT retried. ``sleep`` and ``rand`` are injectable so
tests run instantly and deterministically.
"""

from __future__ import annotations

import logging
import random
import time
from collections.abc import Callable
from typing import TypeVar

from .api_client import TransientError

log = logging.getLogger("gamma_ingestion")

T = TypeVar("T")


def retry_transient(
    fn: Callable[[], T],
    attempts: int,
    base_delay: float,
    sleep: Callable[[float], None] = time.sleep,
    rand: Callable[[], float] = random.random,
) -> T:
    """Call ``fn``, retrying on ``TransientError`` up to ``attempts`` total tries.

    Backoff before retry *n* is uniform in ``[0, base_delay * 2**(n-1)]`` (full
    jitter — avoids thundering-herd retries). Re-raises the last ``TransientError``
    once attempts are exhausted.
    """
    last: TransientError
    for attempt in range(1, attempts + 1):
        try:
            return fn()
        except TransientError as exc:
            last = exc
            if attempt >= attempts:
                break
            delay = base_delay * (2 ** (attempt - 1)) * rand()
            log.warning(
                "transient failure (attempt %d/%d), retrying in %.2fs: %s",
                attempt,
                attempts,
                delay,
                exc,
            )
            sleep(delay)
    raise last
