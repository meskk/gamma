// Optimistic like TOGGLE (C1 + ADR 0012): ONE place for the server-state
// hydration, the optimistic override + revert, the single-flight guard, and the
// Wire<NewInteraction> body construction — for POST likes and COMMENT likes.
//
// State model: the server row (`liked_by_me` / `like_count`, passed in by the
// caller from the fetched contract type) is the BASELINE; the hook keeps only a
// local override on top of it. The override resets when navigating to another
// target, so a fresh fetch always wins. The displayed count derives from the
// baseline: +1 / −1 only when the override actually diverges from the server
// state — never a client-side counter that can drift.

import { useEffect, useRef, useState } from "react";

import type { NewInteraction } from "@contract/NewInteraction";

import { apiFetch } from "@/lib/api";
import type { Wire } from "@/lib/wire";

/** Exactly one of the ids — a post like or a comment like. */
export type LikeTarget = { postId?: string; commentId?: string };

/** The server truth from the fetched row; `null`/`undefined` while loading. */
export type LikeServerState = { liked: boolean; count: number };

export function useLike(target: LikeTarget, token: string, server?: LikeServerState | null) {
  const key = target.postId != null ? `p${target.postId}` : `c${target.commentId}`;
  const [override, setOverride] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  // Reset when navigating between targets — a stale override would mislabel
  // the button on the new post/comment. The ref lets an async failure-revert
  // check whether it still belongs to the mounted target (see toggle()).
  const keyRef = useRef(key);
  keyRef.current = key;
  useEffect(() => setOverride(null), [key]);

  const baseLiked = server?.liked ?? false;
  const baseCount = server?.count ?? 0;
  const liked = override ?? baseLiked;
  const count = baseCount + (liked === baseLiked ? 0 : liked ? 1 : -1);

  async function toggle() {
    if (busy) return; // single-flight: a rapid double-click must not race POST/DELETE
    const startKey = key;
    const next = !liked;
    setOverride(next); // optimistic
    setBusy(true);
    const body: Wire<NewInteraction> = {
      type: "like",
      target_id: null,
      post_id: target.postId != null ? Number(target.postId) : null,
      comment_id: target.commentId != null ? Number(target.commentId) : null,
    };
    try {
      // POST records the like; DELETE retracts it (idempotent 204).
      await apiFetch<void>("/interactions", {
        method: next ? "POST" : "DELETE",
        body,
        token,
      });
    } catch {
      // Revert on failure — but only if the hook still shows the SAME target.
      // After a client-side navigation the reset effect already cleared the
      // override; a late rejection from the OLD target must not write a stale
      // override into the new one.
      if (keyRef.current === startKey) setOverride(!next);
    } finally {
      setBusy(false);
    }
  }

  return { liked, count, toggle, busy };
}
