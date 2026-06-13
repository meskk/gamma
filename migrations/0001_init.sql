-- Phase 1a core schema. Sized for 5–10k users: plain Postgres is sufficient
-- (Rebuild Dossier v5, Appendix A.1). Indexes use plain b-tree/BRIN — NO
-- non-immutable partial predicates (now() is STABLE, not IMMUTABLE).

CREATE TABLE users (
    id                 BIGSERIAL PRIMARY KEY,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    declared_categories TEXT[] NOT NULL DEFAULT '{}',
    -- Bot gate v_i: manual early, heuristic later, KYC in Phase 2. Hard {0,1}.
    bot_gate_v         BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE posts (
    id               BIGSERIAL PRIMARY KEY,
    author_id        BIGINT NOT NULL REFERENCES users(id),
    category         TEXT,
    body             TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    popularity_score DOUBLE PRECISION NOT NULL DEFAULT 0
);

CREATE INDEX posts_created     ON posts (created_at DESC);
CREATE INDEX posts_cat_created ON posts (category, created_at DESC);
CREATE INDEX posts_popularity  ON posts (popularity_score DESC);

CREATE TABLE follows (
    follower_id BIGINT NOT NULL REFERENCES users(id),
    followee_id BIGINT NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (follower_id, followee_id)
);

CREATE INDEX follows_by_follower ON follows (follower_id);

-- ---------------------------------------------------------------------------
-- Interaction-graph capture — SHIP IN WEEK ONE. Epoch boundaries cannot be
-- reconstructed retroactively, so this is append-only and stamped with the
-- epoch from day one (Dossier Appendix B.1). It feeds both the feed ranker and
-- the Gamma node score. Missing this early is the one mistake you cannot undo.
-- ---------------------------------------------------------------------------
CREATE TABLE interaction_events (
    id         BIGSERIAL PRIMARY KEY,
    actor_id   BIGINT NOT NULL,
    target_id  BIGINT,
    post_id    BIGINT,
    type       SMALLINT NOT NULL,        -- like / comment / follow / share / dwell
    weight     DOUBLE PRECISION NOT NULL,-- edge weight feeding column-normalized M
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    epoch_k    INTEGER NOT NULL
);

CREATE INDEX ie_epoch       ON interaction_events (epoch_k);
CREATE INDEX ie_actor_epoch ON interaction_events (actor_id, epoch_k);
