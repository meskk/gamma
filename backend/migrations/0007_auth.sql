-- Authentication (Phase 1a). Users gain login credentials; sessions are opaque
-- bearer tokens stored only as a SHA-256 hash (a DB leak can't be replayed).
-- Columns are nullable so pre-auth/internal user rows remain valid.
ALTER TABLE users
    ADD COLUMN email         TEXT UNIQUE,
    ADD COLUMN password_hash TEXT;

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX sessions_user ON sessions (user_id);
