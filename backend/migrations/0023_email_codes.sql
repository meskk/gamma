-- One-time email codes for auth recovery: passwordless login and password
-- reset. A short-lived numeric code is emailed to the account owner; only its
-- SHA-256 hash is stored, so a DB leak cannot be replayed (same principle as
-- sessions.token_hash). ONE active code per (email, purpose): requesting a new
-- one overwrites the old, and a per-row cooldown (enforced in the repository)
-- bounds email-bombing. Keyed by EMAIL, not user id, on purpose: the request
-- path must behave identically for unknown addresses, so it never becomes an
-- account-existence oracle.
CREATE TABLE email_codes (
    email       TEXT NOT NULL,
    purpose     TEXT NOT NULL CHECK (purpose IN ('login', 'password_reset')),
    code_hash   TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    attempts    INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (email, purpose)
);

-- Housekeeping sweep of expired codes (mirrors sessions_expires_at).
CREATE INDEX email_codes_expires_at ON email_codes (expires_at);
