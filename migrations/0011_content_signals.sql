-- AI ingestion write-back target. The (later, external) ingestion service reads
-- post ids from the `gamma:ingestion` queue, analyses the content, and writes its
-- results here via the operator-only signals endpoint (see ADR 0006). The feed
-- will consume these signals once their shape is settled; until then rows simply
-- don't exist and the feed falls back to its deterministic ranking.
--
-- `signals` is JSONB on purpose: the pipeline's output (topic, quality, later
-- embeddings, …) can evolve WITHOUT a Rust migration per field. `model_version`
-- records which model produced the row, so a re-analysis can supersede it.
CREATE TABLE content_signals (
    post_id       BIGINT PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
    model_version TEXT        NOT NULL,
    signals       JSONB       NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
