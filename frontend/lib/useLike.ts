// Optimistic like (C1): ONE place for the single-shot guard, the revert on
// failure, and the Wire<NewInteraction> body construction (ActionRail and the
// post-detail page used to build it two different ways). Known UI lie, in ONE
// doc comment: the Post contract carries no per-viewer liked flag, so after a
// reload an already-liked post shows unliked until the viewer interacts. When
// the backend adds `liked_by_me`, the fix is a one-line initial-state
// hydration HERE.

import { useEffect, useState } from "react";

import type { NewInteraction } from "@contract/NewInteraction";

import { apiFetch } from "@/lib/api";
import type { Wire } from "@/lib/wire";

export function useLike(postId: string, token: string) {
  const [liked, setLiked] = useState(false);
  // Reset when navigating between posts — a stale `true` would mislabel the
  // button and make like() early-return on the new post.
  useEffect(() => setLiked(false), [postId]);

  async function like() {
    if (liked) return; // the backend records a like once; there is no un-like yet
    setLiked(true); // optimistic
    const body: Wire<NewInteraction> = {
      type: "like",
      target_id: null,
      post_id: Number(postId),
    };
    try {
      await apiFetch<void>("/interactions", { method: "POST", body, token });
    } catch {
      setLiked(false); // revert on failure
    }
  }

  return { liked, like };
}
