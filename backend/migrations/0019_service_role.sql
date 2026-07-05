-- A dedicated 'service' role for machine identities (MASTERPLAN M2.8) — first
-- consumer: the AI ingestion worker, which until now had to borrow a full
-- operator account. A service principal may write content signals but holds
-- NONE of the human-operator powers (settlement, verification, moderation,
-- contracts). Provisioning (documented in services/ingestion/RUNBOOK.md):
-- register a normal account, then
--   UPDATE users SET role = 'service' WHERE id = <id>;
-- There is deliberately NO role-escalation endpoint in Phase 1a.
ALTER TYPE user_role ADD VALUE IF NOT EXISTS 'service';
