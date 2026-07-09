-- Asset-level media takedown (operator moderation).
--
-- Distinct from POST takedown (posts.hidden_at): media_id is a NON-unique FK, so
-- one asset can back several posts and hiding a post is only a partial handle on
-- illegal media — the asset stays reachable via any other post that references
-- it. A taken-down ASSET is blocked directly, regardless of how many posts point
-- at it, for EVERYONE including the owner (moderation is not owner-exempt).
-- NULL = live (default); a timestamp = taken down.
ALTER TABLE media_assets ADD COLUMN hidden_at TIMESTAMPTZ;
