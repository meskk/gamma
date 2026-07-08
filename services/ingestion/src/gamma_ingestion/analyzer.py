"""The content-analysis seam.

``Analyzer`` is the swappable interface the worker depends on — NOT a concrete
function — mirroring the ledger/transcode seams elsewhere in the project. The real
model (on the Mac Studio / rented GPU) drops in later as a second implementation
with zero worker changes; only a factory (P2) and the model-runtime layer (P18)
change. Since ADR 0009 the analysis output follows a VERSIONED contract: each
analyser declares the ``schema_version`` its dict speaks (the API validates the
typed v1 core on write and rejects unknown top-level keys), alongside the
``model_version`` provenance tag it owns. The feed still does not consume the
signals until M2.7.
"""

from __future__ import annotations

import json
import re
from typing import Protocol

import httpx

from .api_client import ApiError, TransientError
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

    @property
    def schema_version(self) -> int:
        """Which ADR-0009 signal contract ``analyze``'s dict follows (v1 core = 1)."""
        ...

    def analyze(self, post: dict) -> dict:
        """Derive signals from a post dict (as returned by ``GET /posts/:id``)."""
        ...


# The heuristic OWNS this label intrinsically — it is the only place its version is
# named, so its output can never be mislabelled as something else. v1 = the ADR 0009
# move: the surface features live under the ``extras`` annex, the typed core stays
# empty (the heuristic cannot honestly claim quality/bot/topics/language).
_HEURISTIC_VERSION = "heuristic-v1"
_HEURISTIC_SCHEMA = 1

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
    declares its own ``model_version``; its output must speak an ADR 0009 schema
    version, like this one does.
    """

    @property
    def model_version(self) -> str:
        return _HEURISTIC_VERSION

    @property
    def schema_version(self) -> int:
        return _HEURISTIC_SCHEMA

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

        # ADR 0009 schema v1: everything the heuristic produces is a surface
        # feature, so it ALL belongs in the free ``extras`` annex — the typed core
        # (quality, bot_likelihood, topics, language, nsfw_likelihood) stays empty
        # because nothing here actually understands the content. The API would
        # reject these keys at the top level (unknown_signal_field), and rightly so.
        return {
            "extras": {
                "kind": "heuristic",
                "has_body": bool(body.strip()),
                "char_count": len(body),
                "word_count": word_count,
                "link_count": link_count,
                "reading_seconds": round(word_count / _WORDS_PER_MINUTE * 60),
                # Pass the author-declared category through untouched (None if
                # absent); the heuristic does not infer topics — that's the real
                # model's job, later.
                "declared_category": post.get("category"),
            }
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


# The reserved top-level key the worker lifts out of ``analyze()``'s dict into
# the write-back envelope (ADR 0009 §3). It is NOT part of the signals document;
# if a worker ever forgot to strip it, the API would reject the write with
# ``unknown_signal_field`` — loud, fail-closed, never silently stored.
EMBEDDING_KEY = "embedding"

# A loose lowercase BCP-47 primary tag, mirroring the API's validation.
_LANGUAGE = re.compile(r"^[a-z][a-z0-9-]{0,33}[a-z0-9]$")

# What the prompt asks for — and the hard ceiling _sanitize enforces even if a
# prompt-injected post talks the model into listing more (the API caps at 16
# and would otherwise reject the whole write).
_MAX_TOPICS_EMITTED = 3

# The judgment prompt. The label space is injected per deployment (the app's
# category set, GAMMA_TOPIC_LABELS) — per ADR 0009 the label SPACE is this
# analyser's contract, the API only enforces the namespace form.
_SYSTEM_PROMPT = (
    "You are a content analyst for a social platform. Analyse the post and "
    "answer with STRICT JSON only — no prose, no code fences — exactly these "
    "keys:\n"
    '{"quality": <0..1>, "bot_likelihood": <0..1>, "nsfw_likelihood": <0..1>, '
    '"language": "<BCP-47 primary tag like de, en>", "topics": [<0..3 labels>]}\n'
    "quality: how substantive/original/effortful the text is (0 = spam-grade, "
    "1 = excellent). bot_likelihood: how likely this SINGLE text is machine-"
    "generated spam (repetition, scam patterns, link farming). nsfw_likelihood: "
    "sexual/graphic content. topics: only labels from this exact list that "
    "clearly apply: {labels}."
)


class ModelAnalyzer:
    """The real analyser (M2.4c): judgments from an instruct LLM, embeddings from
    an encoder — both reached over OpenAI-compatible HTTP endpoints served on the
    GPU box (e.g. vLLM for the LLM, text-embeddings-inference for the encoder).

    Inference is HTTP on purpose: the worker carries ZERO ML dependencies (httpx
    is already here), CI stays hardware-free (mock transports), and the ingestion
    image stays slim — the GPU box is a serving concern, not a worker concern
    (RUNBOOK §6). Provenance stays no-knob: ``model_version`` is derived from the
    model ids the endpoints THEMSELVES report via ``/v1/models`` at startup, so
    the label cannot drift from what actually produced the signals. Construction
    fails fast if an endpoint is unreachable or serves an ambiguous model list —
    mirroring ``Worker.prime()``'s fail-at-startup philosophy.
    """

    def __init__(self, config: Config, transport: httpx.BaseTransport | None = None) -> None:
        self._topics = config.model_topic_labels
        self._max_input_chars = config.model_max_input_chars
        self._llm_base = config.model_base_url
        self._embed_base = config.embed_base_url
        self._client = httpx.Client(timeout=config.model_timeout_seconds, transport=transport)
        llm_id = self._served_model_id(self._llm_base)
        if self._embed_base == self._llm_base:
            embed_id = llm_id
        else:
            embed_id = self._served_model_id(self._embed_base)
        self._llm_id = llm_id
        self._embed_id = embed_id
        self._version = f"llm:{llm_id}+emb:{embed_id}"
        self._system_prompt = _SYSTEM_PROMPT.replace("{labels}", ", ".join(self._topics))

    @property
    def model_version(self) -> str:
        return self._version

    @property
    def schema_version(self) -> int:
        return 1

    def analyze(self, post: dict) -> dict:
        body = (post.get("body") or "").strip()
        if not body:
            # Media-only/empty posts: a judgment or embedding of nothing would be
            # noise pretending to be signal. Store the honest minimum.
            return {"extras": {"kind": "llm", "note": "empty_body"}}
        # Bound the input for BOTH calls: an over-context body would 400/413 at
        # the inference server — a PERMANENT error that dead-letters a healthy
        # post. A judgment over the prefix beats no signals at all.
        text = body[: self._max_input_chars]
        judgment = self._judge(text)
        if len(text) < len(body):
            judgment["extras"]["input_truncated_from_chars"] = len(body)
        judgment[EMBEDDING_KEY] = self._embed(text)
        return judgment

    # ── inference calls ──────────────────────────────────────────────────────

    def _served_model_id(self, base: str) -> str:
        data = self._request("GET", f"{base}/v1/models").get("data") or []
        ids = [m.get("id") for m in data if isinstance(m, dict) and m.get("id")]
        if len(ids) != 1:
            raise ConfigError(
                f"{base}/v1/models must serve exactly ONE model so provenance is "
                f"unambiguous, got {ids!r} — run one endpoint per model (RUNBOOK §6)."
            )
        return str(ids[0])

    def _judge(self, body: str) -> dict:
        resp = self._request(
            "POST",
            f"{self._llm_base}/v1/chat/completions",
            json={
                "model": self._llm_id,
                "temperature": 0,
                "max_tokens": 300,
                "response_format": {"type": "json_object"},
                "messages": [
                    {"role": "system", "content": self._system_prompt},
                    {"role": "user", "content": body},
                ],
            },
        )
        try:
            content = resp["choices"][0]["message"]["content"]
        except (KeyError, IndexError, TypeError) as exc:
            raise ApiError(f"LLM response missing choices/message: {resp!r:.200}") from exc
        return self._sanitize(content)

    def _embed(self, body: str) -> list[float]:
        resp = self._request(
            "POST",
            f"{self._embed_base}/v1/embeddings",
            json={"model": self._embed_id, "input": body},
        )
        try:
            vector = resp["data"][0]["embedding"]
        except (KeyError, IndexError, TypeError) as exc:
            raise ApiError(f"embedding response missing data[0].embedding: {resp!r:.200}") from exc
        if not isinstance(vector, list) or not vector:
            raise ApiError("embedding endpoint returned an empty/invalid vector")
        return [float(x) for x in vector]

    def _request(self, method: str, url: str, **kwargs: object) -> dict:
        """One inference-server round trip, mapped onto the worker's retry model:
        network problems and 5xx are transient (retried with backoff, then DLQ);
        anything else is permanent for this post."""
        try:
            resp = self._client.request(method, url, **kwargs)  # type: ignore[arg-type]
        except httpx.HTTPError as exc:
            raise TransientError(f"{method} {url}: transport error: {exc}") from exc
        if resp.status_code >= 500:
            raise TransientError(f"{method} {url} failed: {resp.status_code}")
        if resp.status_code != 200:
            raise ApiError(f"{method} {url} failed: {resp.status_code} {resp.text:.200}")
        payload: dict = resp.json()
        return payload

    # ── output sanitation ─────────────────────────────────────────────────────

    def _sanitize(self, content: str) -> dict:
        """Parse the LLM's JSON and coerce it into the ADR 0009 v1 core. Strict on
        SHAPE (unparseable JSON is a permanent, DLQ-able failure — temperature 0
        makes a retry pointless), lenient on VALUES: float noise is clamped into
        [0, 1], anything non-conforming is dropped rather than guessed, and topics
        are filtered to the configured label space. What the model literally said
        survives under ``extras.raw`` for operator audits."""
        text = content.strip()
        if text.startswith("```"):
            text = text.strip("`\n")
            text = text.removeprefix("json").strip()
        try:
            raw = json.loads(text)
        except ValueError as exc:
            raise ApiError(f"LLM returned unparseable JSON: {content!r:.200}") from exc
        if not isinstance(raw, dict):
            raise ApiError(f"LLM returned non-object JSON: {content!r:.200}")

        out: dict = {"extras": {"kind": "llm", "raw": raw}}
        for key in ("quality", "bot_likelihood", "nsfw_likelihood"):
            score = _unit_score(raw.get(key))
            if score is not None:
                out[key] = score
        language = raw.get("language")
        if isinstance(language, str) and _LANGUAGE.fullmatch(language.strip().lower()):
            out["language"] = language.strip().lower()
        topics = raw.get("topics")
        if isinstance(topics, list):
            allowed = set(self._topics)
            seen: dict[str, None] = {}
            for item in topics:
                if isinstance(item, str):
                    label = item.strip().lower()
                    if label in allowed and label not in seen:
                        seen[label] = None
            if seen:
                # The prompt asks for at most 3; enforce it here too — the post
                # body is user content inside the prompt, and a prompt-injected
                # "list every label" answer must not blow the API's topics cap
                # (400 invalid_topics would dead-letter the post).
                out["topics"] = list(seen)[:_MAX_TOPICS_EMITTED]
        return out


def _unit_score(value: object) -> float | None:
    """A [0,1] score, or None if the model produced something non-numeric.
    Clamps float noise (1.02 → 1.0) instead of failing the whole post."""
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return min(1.0, max(0.0, float(value)))


def make_analyzer(config: Config) -> Analyzer:
    """Select the analyser implementation from config (``GAMMA_ANALYZER``).

    THE SWAP POINT. Flipping ``GAMMA_ANALYZER=model`` — once the model-runtime layer
    exists (P18) — replaces the heuristic with the real model and the worker never
    changes. Every analyser OWNS its ``model_version`` intrinsically (the heuristic
    reports ``"heuristic-v1"``; the model analyser will report its own weights tag),
    so the provenance stamp can never drift from the code that produced it — there is
    no separate config knob to mislabel it.
    """
    choice = config.analyzer
    if choice == "heuristic":
        return HeuristicAnalyzer()
    if choice == "model":
        # Constructing eagerly probes both endpoints (/v1/models): unreachable or
        # ambiguous serving fails the process AT STARTUP, not on the first post.
        return ModelAnalyzer(config)
    raise ConfigError(f"GAMMA_ANALYZER must be 'heuristic' or 'model', got {choice!r}")
