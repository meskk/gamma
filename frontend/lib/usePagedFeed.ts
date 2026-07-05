// Paginated feed fetching (D1) against the FeedPage cursor contract (B1).
// Accumulation semantics differ from a plain fetch (append + dedupe), so this
// is its own hook. Accepts BOTH the FeedPage shape and the legacy bare array,
// so client and backend can deploy independently.

import { useCallback, useEffect, useRef, useState } from "react";

import type { FeedPage } from "@contract/FeedPage";
import type { Post } from "@contract/Post";

import { apiFetch } from "@/lib/api";

const PAGE_SIZE = 20;

function asPage(resp: FeedPage | Post[]): { items: Post[]; next: string | null } {
  if (Array.isArray(resp)) return { items: resp, next: null };
  return { items: resp.items, next: resp.next_cursor ?? null };
}

export function usePagedFeed(userId: string, token: string) {
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  // Generation counter: identity changes and reloads invalidate in-flight
  // responses (the shared stale-guard pattern). Cursor + single-flight live in
  // refs — they are pagination state, not render state.
  const gen = useRef(0);
  const nextCursor = useRef<string | null>(null);
  const inFlight = useRef(false);

  useEffect(() => {
    const g = ++gen.current;
    inFlight.current = false;
    nextCursor.current = null;
    setPosts(null);
    setError(null);
    apiFetch<FeedPage | Post[]>(`/users/${userId}/feed?limit=${PAGE_SIZE}`, { token })
      .then((resp) => {
        if (g !== gen.current) return;
        const page = asPage(resp);
        nextCursor.current = page.next;
        setPosts(page.items);
      })
      .catch(() => {
        if (g === gen.current) setError("Feed konnte nicht geladen werden.");
      });
  }, [userId, token, tick]);

  /** Fetch the next page, if any. Single-flight; a no-op once exhausted, so
   * callers can fire it optimistically near the end of the list. */
  const loadMore = useCallback(() => {
    const cursor = nextCursor.current;
    if (!cursor || inFlight.current) return;
    inFlight.current = true;
    const g = gen.current;
    apiFetch<FeedPage | Post[]>(
      `/users/${userId}/feed?limit=${PAGE_SIZE}&cursor=${encodeURIComponent(cursor)}`,
      { token },
    )
      .then((resp) => {
        if (g !== gen.current) return;
        const page = asPage(resp);
        nextCursor.current = page.next;
        setPosts((prev) => {
          // Dedupe against everything already shown — belt-and-suspenders
          // against cursor drift; the track keys by post id.
          const seen = new Set((prev ?? []).map((p) => String(p.id)));
          return [...(prev ?? []), ...page.items.filter((p) => !seen.has(String(p.id)))];
        });
      })
      .catch(() => {
        // stale_cursor or a transient failure: stop paging quietly — the list
        // stays usable and a reload() starts a fresh ranking.
        if (g === gen.current) nextCursor.current = null;
      })
      .finally(() => {
        if (g === gen.current) inFlight.current = false;
      });
  }, [userId, token]);

  const reload = useCallback(() => setTick((t) => t + 1), []);

  return { posts, error, loadMore, reload };
}
