import pytest

from gamma_ingestion.config import Config, ConfigError

# The only required values; everything else has a localhost-friendly default.
REQUIRED = {
    "GAMMA_OPERATOR_EMAIL": "op@example.com",
    "GAMMA_OPERATOR_PASSWORD": "pw",
}


def test_defaults_when_only_required_set():
    c = Config.from_env(dict(REQUIRED))
    assert c.analyzer == "heuristic"
    assert c.model_version == "heuristic-v0"
    assert c.redis_url == "redis://localhost:6379"
    assert c.queue_key == "gamma:ingestion"
    assert c.api_base_url == "http://localhost:8080/v1"
    assert c.poll_timeout_seconds == 5.0
    assert c.request_timeout_seconds == 10.0


def test_missing_operator_credentials_raises():
    with pytest.raises(ConfigError):
        Config.from_env({})


def test_overrides_are_read():
    env = dict(
        REQUIRED,
        GAMMA_ANALYZER="model",
        GAMMA_MODEL_VERSION="real-v2",
        REDIS_URL="redis://r:6379",
        GAMMA_POLL_TIMEOUT_SECONDS="2",
    )
    c = Config.from_env(env)
    assert c.analyzer == "model"
    assert c.model_version == "real-v2"
    assert c.redis_url == "redis://r:6379"
    assert c.poll_timeout_seconds == 2.0


def test_api_base_url_trailing_slash_is_stripped():
    c = Config.from_env(dict(REQUIRED, GAMMA_API_BASE_URL="http://x/v1/"))
    assert c.api_base_url == "http://x/v1"


def test_non_numeric_timeout_raises():
    with pytest.raises(ConfigError):
        Config.from_env(dict(REQUIRED, GAMMA_POLL_TIMEOUT_SECONDS="abc"))


def test_retry_knobs_default_and_override():
    c = Config.from_env(dict(REQUIRED))
    assert c.retry_attempts == 3
    assert c.retry_base_delay_seconds == 0.5

    c = Config.from_env(
        dict(REQUIRED, GAMMA_RETRY_ATTEMPTS="5", GAMMA_RETRY_BASE_DELAY_SECONDS="0.1")
    )
    assert c.retry_attempts == 5
    assert c.retry_base_delay_seconds == 0.1


def test_non_integer_retry_attempts_raises():
    with pytest.raises(ConfigError):
        Config.from_env(dict(REQUIRED, GAMMA_RETRY_ATTEMPTS="3.5"))


def test_zero_retry_attempts_raises():
    # attempts < 1 is nonsensical (0 tries) — fail fast at config load, not later.
    with pytest.raises(ConfigError):
        Config.from_env(dict(REQUIRED, GAMMA_RETRY_ATTEMPTS="0"))


def test_dead_letter_key_derives_from_queue_key():
    # Default: "<queue_key>:dead".
    c = Config.from_env(dict(REQUIRED, GAMMA_INGESTION_QUEUE="custom:q"))
    assert c.dead_letter_key == "custom:q:dead"
    # Explicit override wins.
    c = Config.from_env(dict(REQUIRED, GAMMA_INGESTION_DEAD_QUEUE="some:other:dead"))
    assert c.dead_letter_key == "some:other:dead"
