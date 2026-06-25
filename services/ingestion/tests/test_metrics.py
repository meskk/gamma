from gamma_ingestion.metrics import Metrics


def test_records_outcomes():
    m = Metrics()
    m.record_outcome("written")
    m.record_outcome("written")
    m.record_outcome("skipped_missing")
    assert m.written == 2
    assert m.skipped_missing == 1
    assert m.total == 3


def test_records_failure_with_and_without_dead_letter():
    m = Metrics()
    m.record_failure(dead_lettered=True)
    m.record_failure(dead_lettered=False)
    assert m.failed == 2
    assert m.dead_lettered == 1


def test_unknown_outcome_is_ignored():
    m = Metrics()
    m.record_outcome("weird")
    assert m.total == 0


def test_as_dict_shape():
    m = Metrics(written=1, skipped_missing=2, failed=3, dead_lettered=1)
    assert m.as_dict() == {
        "total": 6,
        "written": 1,
        "skipped_missing": 2,
        "failed": 3,
        "dead_lettered": 1,
    }
