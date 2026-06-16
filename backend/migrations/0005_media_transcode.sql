-- Transcoding output for media assets. The raw upload stays under object_key;
-- the HLS rendition (manifest + segments) is uploaded under object_key/hls/ and
-- the manifest path recorded here. transcode_status tracks the pipeline.
ALTER TABLE media_assets
    ADD COLUMN hls_manifest_key TEXT,
    ADD COLUMN transcode_status TEXT NOT NULL DEFAULT 'none';  -- none | done | failed
