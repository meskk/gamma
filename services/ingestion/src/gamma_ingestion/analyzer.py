"""The content-analysis seam.

``Analyzer`` is the swappable interface the worker depends on — NOT a concrete
function — mirroring the ledger/transcode seams elsewhere in the project. The real
model (on the Mac Studio / rented GPU) drops in later as a second implementation
with zero worker changes; only a factory (P2) and the model-runtime layer (P18)
change. The analysis output is an opaque ``dict`` by design: no signal SHAPE is
prescribed here, and the feed does not consume it yet (ADR 0006).
"""

from __future__ import annotations

import re
from typing import Protocol


class Analyzer(Protocol):
    """A content analyser: derives signals from a post and names its own version.

    Each analyser OWNS its ``model_version`` so the provenance tag written to
    ``content_signals.model_version`` can never drift from the implementation that
    produced it — swapping the implementation swaps the label atomically.
    """

    @property
    def model_version(self) -> str:
        """Provenance tag for the signals this analyser produces."""
        ...

    def analyze(self, post: dict) -> dict:
        """Derive signals from a post dict (as returned by ``GET /posts/:id``)."""
        ...


# A run of non-whitespace counts as a word — good enough for a length heuristic,
# and deterministic across inputs.
_WORD = re.compile(r"\S+")
_URL = re.compile(r"https?://\S+")
# Average adult reading speed, words per minute; only used for a rough estimate.
_WORDS_PER_MINUTE = 200


class HeuristicAnalyzer:
    """PHASE-1a PLACEHOLDER. This is NOT a model. It computes cheap, deterministic,
    explainable surface features over a post's text so the end-to-end pipeline
    (queue -> fetch -> analyse -> write-back) is real and testable before the actual
    AI service exists. Keeping it deterministic also keeps it honest: nothing here
    pretends to understand the content, and the feed does not consume these signals
    yet (ADR 0006). The real model replaces this with another ``Analyzer`` impl and
    declares its own ``model_version``; the signal *shape* is deliberately minimal
    until then.
    """

    def __init__(self, model_version: str = "heuristic-v0") -> None:
        self._model_version = model_version

    @property
    def model_version(self) -> str:
        return self._model_version

    def analyze(self, post: dict) -> dict:
        """Pure and deterministic: same post in, same signals out. The returned dict
        is stored verbatim under ``content_signals.signals``."""
        body = post.get("body") or ""
        words = _WORD.findall(body)
        word_count = len(words)
        link_count = len(_URL.findall(body))

        return {
            "kind": "heuristic",
            "has_body": bool(body.strip()),
            "char_count": len(body),
            "word_count": word_count,
            "link_count": link_count,
            "reading_seconds": round(word_count / _WORDS_PER_MINUTE * 60),
            # Pass the author-declared category through untouched (None if absent); the
            # heuristic does not infer topics — that's the real model's job, later.
            "declared_category": post.get("category"),
        }
