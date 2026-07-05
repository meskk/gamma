"""Liveness endpoint (MASTERPLAN M4.1 / prep-plan P9b).

A tiny stdlib HTTP server on a daemon thread answering ``200 ok`` on
``/healthz`` while the process is alive. LIVENESS only, by design: it says
"the process is up", not "the queue is drained" — the worker's own structured
metrics cover progress. Readiness beyond this is a later concern; the compose
healthcheck (M4.3) and the Dockerfile HEALTHCHECK probe this endpoint.
"""

from __future__ import annotations

import http.server
import logging
import threading

logger = logging.getLogger(__name__)


class _Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 (stdlib API name)
        if self.path == "/healthz":
            body = b"ok"
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_error(404)

    def log_message(self, *_args: object) -> None:
        """Silence per-request access logs — probes fire every few seconds."""


def start_health_server(port: int) -> http.server.HTTPServer:
    """Bind ``0.0.0.0:port`` (``port`` may be 0 for an ephemeral test port) and
    serve on a daemon thread. Returns the server so callers/tests can read the
    bound port and shut it down."""
    server = http.server.HTTPServer(("0.0.0.0", port), _Handler)
    thread = threading.Thread(target=server.serve_forever, name="healthz", daemon=True)
    thread.start()
    logger.info("healthz listening on port %d", server.server_address[1])
    return server
