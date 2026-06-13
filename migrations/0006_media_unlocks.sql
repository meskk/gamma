-- Paid content unlock (Dossier §4.3, the content marketplace). A creator sets a
-- PT price on an asset; a viewer pays it once to gain permanent access. Price 0
-- means free (open to everyone). media_unlocks records who is entitled.
ALTER TABLE media_assets
    ADD COLUMN unlock_price BIGINT NOT NULL DEFAULT 0 CHECK (unlock_price >= 0);

CREATE TABLE media_unlocks (
    viewer_id   BIGINT NOT NULL REFERENCES users(id),
    asset_id    BIGINT NOT NULL REFERENCES media_assets(id),
    unlocked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (viewer_id, asset_id)
);
