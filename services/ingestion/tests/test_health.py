"""The liveness endpoint: 200 on /healthz, 404 elsewhere, clean shutdown."""

from __future__ import annotations

import urllib.error
import urllib.request

import pytest

from gamma_ingestion.config import Config, ConfigError
from gamma_ingestion.health import start_health_server

CREDS = {"GAMMA_OPERATOR_EMAIL": "op@example.com", "GAMMA_OPERATOR_PASSWORD": "pw"}


def test_healthz_answers_ok_and_404_elsewhere() -> None:
    server = start_health_server(0)  # ephemeral port: parallel-safe
    port = server.server_address[1]
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/healthz", timeout=2) as resp:
            assert resp.status == 200
            assert resp.read() == b"ok"
        with pytest.raises(urllib.error.HTTPError) as err:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/other", timeout=2)
        assert err.value.code == 404
    finally:
        server.shutdown()


def test_health_port_config_parses_and_validates() -> None:
    assert Config.from_env({**CREDS}).health_port == 8081
    assert Config.from_env({**CREDS, "GAMMA_HEALTH_PORT": "0"}).health_port == 0
    with pytest.raises(ConfigError):
        Config.from_env({**CREDS, "GAMMA_HEALTH_PORT": "70000"})
    with pytest.raises(ConfigError):
        Config.from_env({**CREDS, "GAMMA_HEALTH_PORT": "abc"})
