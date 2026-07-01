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


# The heuristic OWNS this label intrinsically — it is the only place its version is
# named, so its output can never be mislabelled as something else.
_HEURISTIC_VERSION = "heuristic-v0"

# A run of non-whitespace counts as a word — good enough for a length heuristic on
# whitespace-delimited scripts, and deterministic across inputs.
_WORD = re.compile(r"\S+")
_URL = re.compile(r"https?://\S+")
# CJK (and other scriptio-continua) codepoints: written without spaces, so a
# whitespace tokeniser sees a long essay as ONE "word". Count these codepoints
# individually — roughly one per written "word"/character — so word_count and the
# reading-time estimate are not absurdly (0/1) low for such text. Covers CJK
# Unified Ideographs (+ Ext-A), Hiragana, Katakana, and Hangul syllables. This is a
# deliberately coarse length heuristic, NOT linguistic word segmentation.
_CJK = re.compile(
    r"[぀-ヿ㐀-䶿一-鿿가-힯豈-﫿]"
)
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
        is stored verbatim under ``content_signals.signals``.

        Word counting is script-aware enough not to collapse an unspaced CJK essay to
        one "word": a whitespace token that contains CJK codepoints contributes one
        unit PER CJK codepoint (plus one for any non-CJK remainder), so ``word_count``
        and ``reading_seconds`` scale with a Chinese/Japanese/Korean post's real
        length instead of reporting ``1`` word / ``0`` seconds. This stays a coarse
        length heuristic, not linguistic segmentation — the real model does that."""
        body = post.get("body") or ""
        word_count = self._count_words(body)
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

    @staticmethod
    def _count_words(body: str) -> int:
        """Script-aware word count for the length heuristic.

        Whitespace-delimited scripts (Latin, Cyrillic, …) count as one word per
        non-whitespace run. Scriptio-continua scripts (CJK, Hangul) have no spaces, so
        each CJK codepoint counts as one word; a whitespace token holding CJK counts
        its CJK codepoints, plus one more if it also has non-CJK content (e.g. a
        mixed-script token). Deterministic and dependency-free."""
        count = 0
        for token in _WORD.findall(body):
            cjk = len(_CJK.findall(token))
            if cjk:
                # Each CJK codepoint ≈ one word; +1 if the token also has other chars.
                count += cjk + (1 if len(token) > cjk else 0)
            else:
                count += 1
        return count


def make_analyzer(config: Config) -> Analyzer:
    """Select the analyser implementation from config (``GAMMA_ANALYZER``).

    THE SWAP POINT. Flipping ``GAMMA_ANALYZER=model`` — once the model-runtime layer
    exists (P18) — replaces the heuristic with the real model and the worker never
    changes. Every analyser OWNS its ``model_version`` intrinsically (the heuristic
    reports ``"heuristic-v0"``; the model analyser will report its own weights tag),
    so the provenance stamp can never drift from the code that produced it — there is
    no separate config knob to mislabel it.
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
