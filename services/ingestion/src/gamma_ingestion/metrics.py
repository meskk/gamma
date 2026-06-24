"""Per-outcome counters for the ingestion loop.

Cheap, dependency-free observability: how many posts were written, skipped (gone),
failed, and dead-lettered. The worker emits these as a structured summary log
periodically and at shutdown. A Prometheus exporter is a later upgrade (a backend
TODO in CLAUDE.md); these counter names are the stable surface it would expose.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class Metrics:
    written: int = 0
    skipped_missing: int = 0
    failed: int = 0
    dead_lettered: int = 0

    def record_outcome(self, outcome: str) -> None:
        """Count a successful ``process`` result (``written`` / ``skipped_missing``)."""
        if outcome == "written":
            self.written += 1
        elif outcome == "skipped_missing":
            self.skipped_missing += 1

    def record_failure(self, dead_lettered: bool) -> None:
        """Count a post that raised; ``dead_lettered`` says whether it was quarantined."""
        self.failed += 1
        if dead_lettered:
            self.dead_lettered += 1

    @property
    def total(self) -> int:
        return self.written + self.skipped_missing + self.failed

    def as_dict(self) -> dict[str, int]:
        return {
            "total": self.total,
            "written": self.written,
            "skipped_missing": self.skipped_missing,
            "failed": self.failed,
            "dead_lettered": self.dead_lettered,
        }
