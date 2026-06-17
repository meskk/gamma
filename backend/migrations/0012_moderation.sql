-- Minimal moderation (Phase 1a). Users report posts; an operator soft-hides
-- ("takes down") a post, which removes it from the feed and public reads. The
-- soft-hide is a nullable timestamp — reversible (restore) and auditable. The gem
-- math is deliberately NOT touched here: whether a taken-down post's prior
-- engagement still earns is a separate economic-policy decision.
ALTER TABLE posts ADD COLUMN hidden_at TIMESTAMPTZ;

CREATE TABLE post_reports (
    id          BIGSERIAL PRIMARY KEY,
    post_id     BIGINT      NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    reporter_id BIGINT      NOT NULL REFERENCES users(id),
    reason      TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One report per user per post: re-reporting is idempotent, not amplification.
    UNIQUE (post_id, reporter_id)
);

CREATE INDEX post_reports_post ON post_reports (post_id);

-- Visible-post lookups (feed/list) filter on hidden_at IS NULL — index that path.
CREATE INDEX posts_visible_recent ON posts (created_at DESC) WHERE hidden_at IS NULL;
