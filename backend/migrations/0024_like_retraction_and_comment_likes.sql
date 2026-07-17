-- Like lifecycle on top of the append-only interaction journal (ADR 0012).
--
-- 1. `retracted_at` — an un-like VOIDS the event instead of deleting it: the
--    journal stays append-only for audit/history, and `edges_for_epoch` (the
--    authoritative settlement read) skips voided rows. An un-like before the
--    epoch settles removes the edge; after settlement it is display-only —
--    settled payouts are never clawed back (settlement reads edges at settle
--    time by design). Re-liking within the same epoch un-voids the original
--    row via ON CONFLICT DO UPDATE, so the dedup index still caps the weight
--    an actor can contribute per (type, target, epoch) at one event.
--
-- 2. `comment_id` — likes can now target a comment (the edge resolves to the
--    comment's author). The dedup key gains the column; NULLS NOT DISTINCT
--    keeps exact-repeat collapsing intact for every target shape. The FK is
--    NO ACTION like 0015: an append-only economic record must not lose its
--    referent. NOTE: comments.post_id is ON DELETE CASCADE (0014), so this FK
--    makes a hard post-delete fail loudly instead of silently cascading away
--    economic rows — acceptable, posts are soft-hidden (hidden_at), never
--    hard-deleted.

ALTER TABLE interaction_events
    ADD COLUMN comment_id BIGINT REFERENCES comments (id),
    ADD COLUMN retracted_at TIMESTAMPTZ;

DROP INDEX interaction_events_dedup;
CREATE UNIQUE INDEX interaction_events_dedup
    ON interaction_events (actor_id, type, epoch_k, target_id, post_id, comment_id)
    NULLS NOT DISTINCT;

-- Read-side aggregates (like counts, liked-by-viewer) are computed live from
-- the journal — one source of truth, no denormalised counter to drift. These
-- partial indexes make the per-post/per-comment lookups index-only; a column
-- IS NULL predicate is immutable-safe (unlike now(), see 0001).
CREATE INDEX ie_post_type_active ON interaction_events (post_id, type, actor_id)
    WHERE retracted_at IS NULL;
CREATE INDEX ie_comment_type_active ON interaction_events (comment_id, type, actor_id)
    WHERE retracted_at IS NULL;
