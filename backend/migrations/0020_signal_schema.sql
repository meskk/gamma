-- ADR 0009: the versioned signal schema.
--
-- schema_version records which CONTRACT the `signals` JSONB follows;
-- model_version records WHO produced it. 0 = legacy/no contract (pre-ADR rows
-- and writers that omit the field); 1 = the typed v1 core (validated by the
-- API on write). Consumers gate on schema_version, never on model_version.
ALTER TABLE content_signals
    ADD COLUMN schema_version SMALLINT NOT NULL DEFAULT 0;

-- Embeddings live next door, not inside the signals JSONB (ADR 0009 §3):
-- Phase-2 infrastructure (similarity/personalization) with no consumer today,
-- so deliberately no index yet either — plain REAL[] is lossless to migrate
-- (e.g. to pgvector) when a consumer arrives. Written through the same
-- write-back endpoint; one current row per post, like the signals row.
CREATE TABLE post_embeddings (
    post_id       BIGINT PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
    model_version TEXT        NOT NULL,
    dim           SMALLINT    NOT NULL,
    embedding     REAL[]      NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
