// Launch feature flags (MASTERPLAN P-1): hidden features stay in the code and
// come back with a flag flip — nothing is deleted. NEXT_PUBLIC_* values are
// inlined at build time, so flipping one is a rebuild, not a code change.
export const FEATURES = {
  /** Tip button in the reel rail — the backend feature doesn't exist yet. */
  tips: process.env.NEXT_PUBLIC_FEATURE_TIPS === "true",
  /** Save button — local-only today; hidden until server-side saving exists. */
  saves: process.env.NEXT_PUBLIC_FEATURE_SAVES === "true",
  /** Gem-priced unlocks at composition. The launch model for paid content is
   * the Private Area (P-4), not the gem unlock. */
  gemUnlock: process.env.NEXT_PUBLIC_FEATURE_GEM_UNLOCK === "true",
} as const;
