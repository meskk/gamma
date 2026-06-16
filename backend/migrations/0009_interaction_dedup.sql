-- Anti-gaming: an interaction graph that sums edge weights additively lets a
-- single actor inflate their score by repeating the same interaction. Within one
-- epoch, an (actor, type, target, post) tuple is now idempotent — repeating an
-- identical like/share/follow/dwell is a no-op, not extra weight. Distinct types
-- (a like AND a comment) remain distinct edges, as the model intends.
--
-- NULLS NOT DISTINCT (Postgres 15+) so a like with no explicit target (target_id
-- NULL, post_id set) still de-duplicates correctly. Append-only is preserved for
-- DISTINCT events; only exact repeats collapse.
CREATE UNIQUE INDEX interaction_events_dedup
    ON interaction_events (actor_id, type, epoch_k, target_id, post_id)
    NULLS NOT DISTINCT;
