-- P-4 / ADR 0011: the private area — creator-chosen access model, entitlements,
-- and the non-custodial purchases audit mirror.
--
-- Money discipline: fiat NEVER touches ledger_entries (the conserved PT
-- journal). `purchases` is an AUDIT MIRROR of provider truth (Stripe events) —
-- not conserved, and it does not claim to be. All amounts are integer cents.

-- Which area a post lives in. Private posts leave every public read path
-- (feed candidates, lists, single reads, comments, media of the post) unless
-- the viewer is entitled or the creator — enforced in repository queries like
-- the hidden_at invariant (A4). Default 'public' keeps every existing post
-- and write path unchanged until the flag-gated feature goes live.
ALTER TABLE posts
    ADD COLUMN area TEXT NOT NULL DEFAULT 'public'
        CHECK (area IN ('public', 'private'));

-- One row per creator: how their private area is accessed (the CREATOR
-- chooses — owner decision 2026-07-08). All four models exist from day 1;
-- the payment STAGES land one at a time (one_time -> subscription -> per_post).
CREATE TABLE private_areas (
    creator_id   BIGINT PRIMARY KEY REFERENCES users(id),
    access_model TEXT NOT NULL DEFAULT 'free'
        CHECK (access_model IN ('free', 'one_time', 'subscription', 'per_post')),
    -- Integer cents; EUR-only in v1 (ADR 0011 §4). No floats on money, ever.
    price_cents  BIGINT NOT NULL DEFAULT 0 CHECK (price_cents >= 0),
    currency     TEXT NOT NULL DEFAULT 'EUR' CHECK (currency = 'EUR'),
    description  TEXT NOT NULL DEFAULT '',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Access materializes as an entitlement, never as a payment lookup at read
-- time. NULL expires_at = permanent (one-time purchase); subscriptions set an
-- expiry that invoice.paid events extend — lapse revokes by itself, no cron.
CREATE TABLE area_entitlements (
    viewer_id  BIGINT NOT NULL REFERENCES users(id),
    creator_id BIGINT NOT NULL REFERENCES users(id),
    source     TEXT NOT NULL CHECK (source IN ('purchase', 'subscription', 'operator')),
    expires_at TIMESTAMPTZ,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (viewer_id, creator_id)
);

-- Audit mirror of provider-side money movement. `status` is deliberately
-- unconstrained TEXT: it mirrors the provider's vocabulary ('paid',
-- 'refunded', ...) and must not fight future provider states.
-- (provider, provider_ref) UNIQUE is the webhook idempotency anchor: an event
-- replay inserts nothing twice.
CREATE TABLE purchases (
    id           BIGSERIAL PRIMARY KEY,
    provider     TEXT NOT NULL,
    provider_ref TEXT NOT NULL,
    viewer_id    BIGINT NOT NULL REFERENCES users(id),
    creator_id   BIGINT NOT NULL REFERENCES users(id),
    kind         TEXT NOT NULL CHECK (kind IN ('one_time', 'subscription', 'per_post')),
    amount_cents BIGINT NOT NULL CHECK (amount_cents >= 0),
    currency     TEXT NOT NULL,
    fee_cents    BIGINT NOT NULL CHECK (fee_cents >= 0),
    status       TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_ref)
);

-- P-5 will read a viewer's purchase history; creators will read their sales.
CREATE INDEX purchases_viewer ON purchases (viewer_id, created_at DESC);
CREATE INDEX purchases_creator ON purchases (creator_id, created_at DESC);
