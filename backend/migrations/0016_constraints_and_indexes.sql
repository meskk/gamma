-- Data-integrity hardening surfaced by the deep review: constrain the enum-like
-- TEXT columns so a typo'd status (e.g. 'reday') can't be persisted and then
-- silently never match, and index the session-expiry lookup that logout/cleanup
-- relies on.
--
-- These match the values the code actually writes (see media/repository.rs, the
-- ledger tx primitives). CHECKs are added on empty tables at migration time in a
-- fresh DB, so there is nothing to backfill.

ALTER TABLE media_assets
    ADD CONSTRAINT media_assets_kind_check
        CHECK (kind IN ('image', 'video', 'audio')),
    ADD CONSTRAINT media_assets_status_check
        CHECK (status IN ('pending', 'ready', 'failed')),
    ADD CONSTRAINT media_assets_transcode_status_check
        CHECK (transcode_status IN ('none', 'done', 'failed'));

ALTER TABLE ledger_entries
    ADD CONSTRAINT ledger_entries_kind_check
        CHECK (kind IN ('mint', 'unlock_debit', 'unlock_credit', 'unlock_burn'));

-- Logout and the expired-session sweep filter on expires_at; index it so both stay
-- cheap as the sessions table grows.
CREATE INDEX sessions_expires_at ON sessions (expires_at);

-- NOTE: gem_balances.user_id and ledger_entries.user_id intentionally have NO FK to
-- users(id): the company fee bucket is the sentinel id 0 (no users row, BIGSERIAL
-- starts at 1), and a burn row has user_id NULL. Adding a users FK would reject
-- both. A dedicated system-accounts table is the proper Phase-1b fix.
