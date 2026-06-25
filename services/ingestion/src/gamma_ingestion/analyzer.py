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

from .config import Config, ConfigError


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


# The heuristic OWNS this label intrinsically — it is never taken from config, so
# GAMMA_MODEL_VERSION can never mislabel heuristic output as something else.
_HEURISTIC_VERSION = "heuristic-v0"

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

    @property
    def model_version(self) -> str:
        return _HEURISTIC_VERSION

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


def make_analyzer(config: Config) -> Analyzer:
    """Select the analyser implementation from config (``GAMMA_ANALYZER``).

    THE SWAP POINT. Flipping ``GAMMA_ANALYZER=model`` — once the model-runtime layer
    exists (P18) — replaces the heuristic with the real model and the worker never
    changes. The heuristic owns its label (``"heuristic-v0"``) intrinsically and is
    NEVER fed ``config.model_version``, so the selector and the provenance tag can't
    drift. ``GAMMA_MODEL_VERSION`` / ``config.model_version`` is reserved for the
    future model analyser's label: a model's version tracks its weights, which change
    without a code change, so it legitimately comes from config (RUNBOOK §6 step 3).
    The pure-code heuristic never reads it.
    """
    choice = config.analyzer
    if choice == "heuristic":
        return HeuristicAnalyzer()
    if choice == "model":
        raise NotImplementedError(
            "GAMMA_ANALYZER=model: the real model analyser is not built yet. This is "
            "the single seam the Mac Studio / rented GPU fills (P18) — construct the "
            "model here (weights path, device, batch size) as an Analyzer impl that "
            "declares its own model_version. Until then run with GAMMA_ANALYZER=heuristic."
        )
    raise ConfigError(f"GAMMA_ANALYZER must be 'heuristic' or 'model', got {choice!r}")
