import pytest

from gamma_ingestion.analyzer import HeuristicAnalyzer, make_analyzer
from gamma_ingestion.config import Config, ConfigError


def analyze(post: dict) -> dict:
    return HeuristicAnalyzer().analyze(post)


def _config(analyzer: str = "heuristic", model_version: str = "heuristic-v0") -> Config:
    return Config(
        redis_url="redis://x",
        queue_key="gamma:ingestion",
        api_base_url="http://x/v1",
        operator_email="op@example.com",
        operator_password="pw",
        model_version=model_version,
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


def test_is_deterministic():
    post = {"id": 4, "body": "the quick brown fox", "category": "nature"}
    assert analyze(post) == analyze(post)


def test_missing_keys_default_safely():
    # A post dict without body/category keys must not raise.
    s = analyze({"id": 5})
    assert s["has_body"] is False
    assert s["declared_category"] is None


def test_model_version_is_owned_by_the_analyzer():
    assert HeuristicAnalyzer().model_version == "heuristic-v0"
    # Overridable, so the future real model declares its own version intrinsically.
    assert HeuristicAnalyzer(model_version="real-model-v1").model_version == "real-model-v1"


def test_factory_builds_heuristic_with_configured_version():
    a = make_analyzer(_config(analyzer="heuristic", model_version="heuristic-v0"))
    assert isinstance(a, HeuristicAnalyzer)
    assert a.model_version == "heuristic-v0"


def test_factory_model_branch_fails_fast_until_built():
    with pytest.raises(NotImplementedError):
        make_analyzer(_config(analyzer="model"))


def test_factory_rejects_unknown_analyzer():
    with pytest.raises(ConfigError):
        make_analyzer(_config(analyzer="bogus"))
