-- Referential integrity for the interaction graph. `interaction_events` was created
-- (0001) without foreign keys so the very first capture tests could use synthetic
-- node ids. Every producer now references real rows (the API derives the actor from
-- the session; settlement tests seed real users/posts), so lock it down: an event
-- may only reference a real actor, an optional real target user, and an optional
-- real post.
--
-- ON DELETE is intentionally the DEFAULT (NO ACTION / restrict), NOT cascade — unlike
-- the post_id references in `comments`/`content_signals`. `interaction_events` is an
-- append-only economic record feeding epoch settlement, so a referenced user or post
-- must not be deletable out from under the ledger's history. Posts are moderated by
-- soft-hide (`hidden_at`), never hard-deleted, so the post FK never blocks a takedown.
-- A future user-erasure path must therefore handle these rows explicitly
-- (anonymize/aggregate) instead of silently dropping economic history.
ALTER TABLE interaction_events
    ADD CONSTRAINT interaction_events_actor_fk
        FOREIGN KEY (actor_id) REFERENCES users (id),
    ADD CONSTRAINT interaction_events_target_fk
        FOREIGN KEY (target_id) REFERENCES users (id),
    ADD CONSTRAINT interaction_events_post_fk
        FOREIGN KEY (post_id) REFERENCES posts (id);
