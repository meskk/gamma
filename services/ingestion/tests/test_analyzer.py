import pytest

from gamma_ingestion.analyzer import HeuristicAnalyzer, make_analyzer
from gamma_ingestion.config import Config, ConfigError


def analyze(post: dict) -> dict:
    """The heuristic's surface features — everything lives under ``extras``
    (ADR 0009 v1: the typed core stays empty because the heuristic cannot
    honestly claim quality/bot/topics/language)."""
    signals = HeuristicAnalyzer().analyze(post)
    assert set(signals) == {"extras"}
    return signals["extras"]


def _config(analyzer: str = "heuristic") -> Config:
    return Config(
        redis_url="redis://x",
        queue_key="gamma:ingestion",
        api_base_url="http://x/v1",
        operator_email="op@example.com",
        operator_password="pw",
        poll_timeout_seconds=1.0,
        request_timeout_seconds=1.0,
        analyzer=analyzer,
    )


def test_empty_body_is_inert():
    s = analyze({"id": 1, "body": None, "category": None})
    assert s["has_body"] is False
    assert s["char_count"] == 0
    assert s["word_count"] == 0
    assert s["link_count"] == 0
    assert s["reading_seconds"] == 0
    assert s["declared_category"] is None
    assert s["kind"] == "heuristic"


def test_counts_words_links_and_passes_category():
    body = "Check https://example.com and http://foo.bar now"
    s = analyze({"id": 2, "body": body, "category": "tech"})
    assert s["has_body"] is True
    assert s["char_count"] == len(body)
    assert s["word_count"] == 5
    assert s["link_count"] == 2
    assert s["declared_category"] == "tech"
    # 5 words / 200 wpm * 60 = 1.5s, rounded -> 2.
    assert s["reading_seconds"] == 2


def test_whitespace_only_body_has_no_content():
    s = analyze({"id": 3, "body": "   \n\t  ", "category": None})
    assert s["has_body"] is False
    assert s["word_count"] == 0


def test_cjk_post_is_not_collapsed_to_one_word():
    # An unspaced CJK essay must not read as 1 word / 0 reading-seconds. Each CJK
    # codepoint counts as a word, so a long passage scales sensibly.
    body = "这是一篇很长的中文文章需要一些时间来阅读" * 5  # 20 CJK chars * 5 = 100
    s = analyze({"id": 10, "body": body, "category": None})
    assert s["has_body"] is True
    assert s["word_count"] == 100
    # 100 words / 200 wpm * 60 = 30s — crucially NOT 0 for a long post.
    assert s["reading_seconds"] == 30


def test_mixed_latin_and_cjk_counts_both():
    # A Latin word plus CJK codepoints in the same post both contribute.
    s = analyze({"id": 11, "body": "hello 世界 world", "category": None})
    # "hello" (1) + "世界" (2 CJK) + "world" (1) = 4.
    assert s["word_count"] == 4


def test_is_deterministic():
    post = {"id": 4, "body": "the quick brown fox", "category": "nature"}
    assert analyze(post) == analyze(post)


def test_missing_keys_default_safely():
    # A post dict without body/category keys must not raise.
    s = analyze({"id": 5})
    assert s["has_body"] is False
    assert s["declared_category"] is None


def test_model_version_is_owned_by_the_analyzer():
    # The heuristic owns its label INTRINSICALLY — there is no constructor override,
    # so config can never mislabel heuristic output.
    assert HeuristicAnalyzer().model_version == "heuristic-v1"


def test_schema_version_is_the_adr_0009_v1_contract():
    # The analyser also owns which CONTRACT its dict speaks; the API validates
    # the v1 core on write and would reject the heuristic's surface features at
    # the top level — hence the extras envelope.
    a = HeuristicAnalyzer()
    assert a.schema_version == 1
    signals = a.analyze({"id": 1, "body": "hello", "category": "tech"})
    assert set(signals) == {"extras"}  # empty typed core, everything in the annex


def test_factory_builds_heuristic_with_its_own_label():
    # The factory has no model-version knob to pass through — the heuristic reports
    # its own intrinsic "heuristic-v1", so the label can't drift from the code.
    a = make_analyzer(_config(analyzer="heuristic"))
    assert isinstance(a, HeuristicAnalyzer)
    assert a.model_version == "heuristic-v1"


def test_factory_model_branch_fails_fast_until_built():
    with pytest.raises(NotImplementedError):
        make_analyzer(_config(analyzer="model"))


def test_factory_rejects_unknown_analyzer():
    with pytest.raises(ConfigError):
        make_analyzer(_config(analyzer="bogus"))
