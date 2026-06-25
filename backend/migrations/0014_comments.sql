-- Comments on a post. The interaction graph already captures a "comment" EDGE
-- (used by the gem weighting); this table stores the comment TEXT the graph never
-- did. ON DELETE CASCADE: removing a post removes its comments.
CREATE TABLE comments (
    id         BIGSERIAL PRIMARY KEY,
    post_id    BIGINT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    author_id  BIGINT NOT NULL REFERENCES users(id),
    body       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX comments_post ON comments (post_id, created_at);
