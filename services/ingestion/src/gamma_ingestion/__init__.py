"""Gamma AI ingestion service.

Consumes newly-created post ids from the ``gamma:ingestion`` Redis queue, reads
each post through the core API, derives content signals, and writes them back via
the operator-only ``PUT /v1/posts/:id/signals`` endpoint. See ADR 0006 for the
seam this plugs into; the service never touches Postgres directly.

Phase 1a note: the analyser is a deterministic heuristic placeholder, NOT a real
model — see ``analyzer.py``. The signal *shape* is intentionally minimal until the
real pipeline lands, and the feed does not consume these signals yet.
"""

__version__ = "0.1.0"
