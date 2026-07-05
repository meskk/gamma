-- Per-account login throttle: consecutive failed-login count per (normalised)
-- email and, from the 5th failure on, an exponentially growing lock window.
-- The policy (free failures, first lock, doubling, cap) lives in code:
-- crates/core-api/src/auth/throttle.rs.
--
-- Keyed by EMAIL, not user id, ON PURPOSE: unknown addresses must throttle
-- exactly like real ones. A lockout that only existing accounts can trigger
-- would be a fresh enumeration oracle ("does this email 429 after 5 tries?"),
-- undoing what the dummy-hash timing equalisation in the login path buys.
CREATE TABLE login_throttle (
    email          TEXT PRIMARY KEY,
    failed_count   INT NOT NULL DEFAULT 0,
    last_failed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until   TIMESTAMPTZ
);

-- The housekeeping sweep deletes rows idle for 24h; this keeps it cheap.
CREATE INDEX login_throttle_last_failed_at ON login_throttle (last_failed_at);
