-- Media assets (Phase 1a). One row per uploaded image/video/audio. The bytes
-- live in object storage under object_key; this table only tracks metadata and
-- lifecycle (pending → ready → failed). object_key is an opaque random path, so
-- storage keys are not enumerable from sequential ids.
CREATE TABLE media_assets (
    id           BIGSERIAL PRIMARY KEY,
    owner_id     BIGINT NOT NULL REFERENCES users(id),
    kind         TEXT NOT NULL,                    -- image | video | audio
    object_key   TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',  -- pending | ready | failed
    size_bytes   BIGINT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX media_assets_owner ON media_assets (owner_id, created_at DESC);
