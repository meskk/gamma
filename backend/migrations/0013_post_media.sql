-- A post can carry one media asset (the creative-marketplace content rail). Nullable:
-- most posts are text-only. Access to the asset is gated by the asset's OWN
-- unlock_price (the paywall lives on media_assets, independent of the post), so
-- attaching media never bypasses entitlement. ON DELETE SET NULL keeps the post if
-- its asset is later removed.
ALTER TABLE posts
    ADD COLUMN media_id BIGINT REFERENCES media_assets(id) ON DELETE SET NULL;
