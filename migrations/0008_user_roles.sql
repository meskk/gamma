-- Role-based authorization (Phase 1a). Adds a coarse role to users so privileged
-- actions (currently epoch settlement) can be locked to operators. Defaults to
-- 'user', so every existing and future row is non-privileged unless explicitly
-- promoted — fail-closed.
--
-- There is no admin UI yet; promote an operator manually in dev with:
--   UPDATE users SET role = 'operator' WHERE id = <id>;
-- A proper admin/role-management flow arrives with Phase 1b.
CREATE TYPE user_role AS ENUM ('user', 'operator');

ALTER TABLE users
    ADD COLUMN role user_role NOT NULL DEFAULT 'user';
