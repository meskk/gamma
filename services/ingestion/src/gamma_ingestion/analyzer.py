"""Content analysis.

PHASE-1a PLACEHOLDER. This is NOT a model. It computes cheap, deterministic,
explainable surface features over a post's text so the end-to-end pipeline
(queue -> fetch -> analyse -> write-back) is real and testable before the actual
AI service exists. Keeping it deterministic also keeps it honest: nothing here
pretends to understand the content, and the feed does not consume these signals
yet (ADR 0006). When the real pipeline lands it replaces this module wholesale and
bumps ``model_version``; the signal *shape* is deliberately minimal until then.
"""

from __future__ import annotations

import re

# A run of non-whitespace counts as a word — good enough for a length heuristic,
# and deterministic across inputs.
_WORD = re.compile(r"\S+")
_URL = re.compile(r"https?://\S+")
# Average adult reading speed, words per minute; only used for a rough estimate.
_WORDS_PER_MINUTE = 200


def analyze(post: dict) -> dict:
    """Derive content signals from a post dict (as returned by ``GET /posts/:id``).

    Returns a JSON-serialisable dict stored verbatim under ``content_signals.signals``.
    Pure and deterministic: same post in, same signals out.
    """
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
