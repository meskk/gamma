"""ModelAnalyzer tests — all inference over httpx.MockTransport, so CI needs no
GPU, no weights, no network: exactly the hardware-free seam M2.4 promised."""

import json

import httpx
import pytest

from gamma_ingestion.analyzer import EMBEDDING_KEY, ModelAnalyzer, make_analyzer
from gamma_ingestion.api_client import ApiError, TransientError
from gamma_ingestion.config import Config, ConfigError

LLM_BASE = "http://llm.test"
EMBED_BASE = "http://embed.test"


def _config(**overrides) -> Config:
    defaults = dict(
        redis_url="redis://x",
        queue_key="gamma:ingestion",
        api_base_url="http://x/v1",
        operator_email="op@example.com",
        operator_password="pw",
        poll_timeout_seconds=1.0,
        request_timeout_seconds=1.0,
        analyzer="model",
        model_base_url=LLM_BASE,
        embed_base_url=EMBED_BASE,
        model_topic_labels=("tech", "sport", "musik"),
    )
    defaults.update(overrides)
    return Config(**defaults)


class FakeInference:
    """An OpenAI-compatible stand-in for vLLM (chat) + TEI (embeddings)."""

    def __init__(
        self,
        judgment: dict | str | None = None,
        llm_models: list[str] | None = None,
        fail_with: int | None = None,
    ):
        self.judgment = judgment if judgment is not None else {}
        self.llm_models = llm_models if llm_models is not None else ["qwen3-8b"]
        self.fail_with = fail_with
        self.calls: list[str] = []
        self.bodies: dict[str, dict] = {}  # last request body per path

    def transport(self) -> httpx.MockTransport:
        return httpx.MockTransport(self._handle)

    def _handle(self, request: httpx.Request) -> httpx.Response:
        self.calls.append(f"{request.method} {request.url}")
        if request.content:
            self.bodies[request.url.path] = json.loads(request.content)
        if self.fail_with is not None and "/models" not in str(request.url):
            return httpx.Response(self.fail_with, text="boom")
        if str(request.url) == f"{LLM_BASE}/v1/models":
            return httpx.Response(
                200, json={"data": [{"id": m} for m in self.llm_models]}
            )
        if str(request.url) == f"{EMBED_BASE}/v1/models":
            return httpx.Response(200, json={"data": [{"id": "bge-m3"}]})
        if str(request.url) == f"{LLM_BASE}/v1/chat/completions":
            content = (
                self.judgment if isinstance(self.judgment, str) else json.dumps(self.judgment)
            )
            return httpx.Response(
                200, json={"choices": [{"message": {"content": content}}]}
            )
        if str(request.url) == f"{EMBED_BASE}/v1/embeddings":
            return httpx.Response(
                200, json={"data": [{"embedding": [0.25, -0.5, 0.125]}]}
            )
        return httpx.Response(404, text=f"unexpected {request.url}")


def test_model_version_derives_from_what_the_endpoints_actually_serve():
    # No-knob provenance: the label comes from /v1/models — the runtime truth —
    # so it cannot drift from whatever is really answering inference calls.
    fake = FakeInference()
    a = ModelAnalyzer(_config(), transport=fake.transport())
    assert a.model_version == "llm:qwen3-8b+emb:bge-m3"
    assert a.schema_version == 1


def test_ambiguous_model_list_fails_construction():
    fake = FakeInference(llm_models=["a", "b"])
    with pytest.raises(ConfigError, match="exactly ONE model"):
        ModelAnalyzer(_config(), transport=fake.transport())


def test_analyze_maps_the_judgment_into_the_v1_core():
    fake = FakeInference(
        judgment={
            "quality": 1.2,  # float noise → clamped to 1.0
            "bot_likelihood": 0.05,
            "nsfw_likelihood": "low",  # non-numeric → dropped, not guessed
            "language": "DE",  # normalized to lowercase
            "topics": ["Tech", "unknown-label", "tech"],  # filtered + deduped
        }
    )
    a = ModelAnalyzer(_config(), transport=fake.transport())
    signals = a.analyze({"id": 1, "body": "Ein längerer Beitrag über Rust."})

    embedding = signals.pop(EMBEDDING_KEY)
    assert embedding == [0.25, -0.5, 0.125]
    assert signals["quality"] == 1.0
    assert signals["bot_likelihood"] == 0.05
    assert "nsfw_likelihood" not in signals
    assert signals["language"] == "de"
    assert signals["topics"] == ["tech"]
    # The model's literal answer survives for operator audits.
    assert signals["extras"]["kind"] == "llm"
    assert signals["extras"]["raw"]["language"] == "DE"


def test_code_fenced_json_is_tolerated():
    fake = FakeInference(judgment='```json\n{"quality": 0.5}\n```')
    a = ModelAnalyzer(_config(), transport=fake.transport())
    signals = a.analyze({"id": 1, "body": "hi there"})
    assert signals["quality"] == 0.5


def test_unparseable_llm_output_is_permanent_not_transient():
    # temperature 0 → a retry would reproduce the same garbage; DLQ it instead.
    fake = FakeInference(judgment="I think this post is quite good!")
    a = ModelAnalyzer(_config(), transport=fake.transport())
    with pytest.raises(ApiError):
        a.analyze({"id": 1, "body": "hi there"})


def test_inference_5xx_is_transient():
    fake = FakeInference(fail_with=503)
    a = ModelAnalyzer(_config(), transport=fake.transport())
    with pytest.raises(TransientError):
        a.analyze({"id": 1, "body": "hi there"})


def test_empty_body_skips_inference_entirely():
    fake = FakeInference()
    a = ModelAnalyzer(_config(), transport=fake.transport())
    calls_after_startup = len(fake.calls)
    signals = a.analyze({"id": 1, "body": "   ", "category": None})
    assert signals == {"extras": {"kind": "llm", "note": "empty_body"}}
    assert len(fake.calls) == calls_after_startup  # no LLM/embedding round trips


def test_chat_request_pins_the_determinism_and_label_contract():
    # The judgment call IS the contract with the LLM: temperature 0 (a retry
    # must reproduce, unparseable output is treated as permanent), JSON mode,
    # the label space in the system prompt, the served model id, and the post
    # body as the user message.
    fake = FakeInference(judgment={"quality": 0.5})
    a = ModelAnalyzer(_config(), transport=fake.transport())
    a.analyze({"id": 1, "body": "hello world"})

    chat = fake.bodies["/v1/chat/completions"]
    assert chat["model"] == "qwen3-8b"
    assert chat["temperature"] == 0
    assert chat["response_format"] == {"type": "json_object"}
    assert chat["messages"][0]["role"] == "system"
    assert "tech, sport, musik" in chat["messages"][0]["content"]
    assert chat["messages"][1] == {"role": "user", "content": "hello world"}

    embed = fake.bodies["/v1/embeddings"]
    assert embed == {"model": "bge-m3", "input": "hello world"}


def test_long_bodies_are_truncated_not_dead_lettered():
    # An over-context body would 400/413 at the inference server — a PERMANENT
    # error that quarantines a healthy post. The analyzer bounds the input for
    # BOTH calls and records the truncation honestly.
    fake = FakeInference(judgment={"quality": 0.5})
    a = ModelAnalyzer(_config(model_max_input_chars=10), transport=fake.transport())
    signals = a.analyze({"id": 1, "body": "x" * 50})
    assert fake.bodies["/v1/chat/completions"]["messages"][1]["content"] == "x" * 10
    assert fake.bodies["/v1/embeddings"]["input"] == "x" * 10
    assert signals["extras"]["input_truncated_from_chars"] == 50


def test_topics_are_capped_even_against_prompt_injection():
    # The post body is user content INSIDE the prompt — a "list every label"
    # injection must not blow the API's topics cap and dead-letter the post.
    fake = FakeInference(judgment={"topics": ["tech", "sport", "musik", "tech"]})
    a = ModelAnalyzer(
        _config(model_topic_labels=("tech", "sport", "musik", "kunst")),
        transport=fake.transport(),
    )
    signals = a.analyze({"id": 1, "body": "hi"})
    assert len(signals["topics"]) <= 3


def test_scores_clamp_both_ends_and_reject_bools():
    fake = FakeInference(
        judgment={"quality": -0.05, "bot_likelihood": True, "nsfw_likelihood": 2}
    )
    a = ModelAnalyzer(_config(), transport=fake.transport())
    signals = a.analyze({"id": 1, "body": "hi"})
    assert signals["quality"] == 0.0  # negative float noise → clamped up
    assert "bot_likelihood" not in signals  # bool is not a score
    assert signals["nsfw_likelihood"] == 1.0  # clamped down


def test_inference_4xx_is_permanent():
    # A 4xx from the inference server (wrong route, over-context) is not
    # retryable — it must be the DLQ path, not a retry storm.
    fake = FakeInference(fail_with=404)
    a = ModelAnalyzer(_config(), transport=fake.transport())
    with pytest.raises(ApiError) as exc_info:
        a.analyze({"id": 1, "body": "hi there"})
    assert not isinstance(exc_info.value, TransientError)


def test_shared_endpoint_is_probed_once():
    # embed base == llm base: one server, one probe, one id for both roles.
    fake = FakeInference()
    a = ModelAnalyzer(
        _config(embed_base_url=LLM_BASE), transport=fake.transport()
    )
    assert a.model_version == "llm:qwen3-8b+emb:qwen3-8b"
    assert fake.calls.count(f"GET {LLM_BASE}/v1/models") == 1


def test_full_chain_process_post_with_the_real_model_analyzer():
    # The whole Python side in one piece: real ModelAnalyzer → process_post →
    # the signals doc that reaches the client is clean (embedding lifted into
    # the envelope, never inside the signals).
    from gamma_ingestion.worker import process_post

    class FakeClient:
        def __init__(self):
            self.written = None

        def get_post(self, post_id):
            return {"id": post_id, "body": "ein echter Beitrag", "category": "tech"}

        def put_signals(
            self, post_id, model_version, schema_version, signals, token, embedding=None
        ):
            self.written = (post_id, model_version, schema_version, signals, embedding)

    fake = FakeInference(judgment={"quality": 0.7, "topics": ["tech"]})
    analyzer = ModelAnalyzer(_config(), transport=fake.transport())
    client = FakeClient()
    assert process_post(client, 9, analyzer, "tok") == "written"

    post_id, model_version, schema_version, signals, embedding = client.written
    assert (post_id, schema_version) == (9, 1)
    assert model_version == "llm:qwen3-8b+emb:bge-m3"
    assert embedding == [0.25, -0.5, 0.125]
    assert "embedding" not in signals
    assert signals["quality"] == 0.7
    assert signals["topics"] == ["tech"]


def test_config_rejects_oversized_topic_labels():
    # The API rejects labels over 64 UTF-8 bytes — catch a miscopied category
    # at startup, not one dead-lettered post at a time. 33 umlauts = 66 bytes.
    with pytest.raises(ConfigError, match="64 UTF-8"):
        Config.from_env(
            {
                "GAMMA_OPERATOR_EMAIL": "op@example.com",
                "GAMMA_OPERATOR_PASSWORD": "pw",
                "GAMMA_ANALYZER": "model",
                "GAMMA_MODEL_BASE_URL": LLM_BASE,
                "GAMMA_TOPIC_LABELS": "tech," + "ü" * 33,
            }
        )


def test_config_requires_endpoint_and_labels_for_model():
    base = dict(
        GAMMA_OPERATOR_EMAIL="op@example.com",
        GAMMA_OPERATOR_PASSWORD="pw",
        GAMMA_ANALYZER="model",
    )
    with pytest.raises(ConfigError, match="GAMMA_MODEL_BASE_URL"):
        Config.from_env({**base, "GAMMA_TOPIC_LABELS": "tech"})
    with pytest.raises(ConfigError, match="GAMMA_TOPIC_LABELS"):
        Config.from_env({**base, "GAMMA_MODEL_BASE_URL": LLM_BASE})


def test_config_normalizes_labels_and_defaults_embed_base():
    cfg = Config.from_env(
        {
            "GAMMA_OPERATOR_EMAIL": "op@example.com",
            "GAMMA_OPERATOR_PASSWORD": "pw",
            "GAMMA_ANALYZER": "model",
            "GAMMA_MODEL_BASE_URL": LLM_BASE + "/",
            "GAMMA_TOPIC_LABELS": " Tech, sport,,SPORT , musik ",
        }
    )
    assert cfg.model_base_url == LLM_BASE
    assert cfg.embed_base_url == LLM_BASE  # defaults to the model endpoint
    assert cfg.model_topic_labels == ("tech", "sport", "musik")


def test_factory_builds_the_model_analyzer():
    # The factory path itself — with a config whose endpoint is unreachable the
    # construction probe MUST fail fast (startup, not first post). A live probe
    # against a mock transport is covered above; here we pin the fail-fast.
    cfg = _config(model_base_url="http://127.0.0.1:9", embed_base_url="http://127.0.0.1:9")
    with pytest.raises(TransientError):
        make_analyzer(cfg)
