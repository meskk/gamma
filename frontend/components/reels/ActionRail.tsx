"use client";

// The right-hand action rail. Wires the actions the backend actually supports —
// the like toggle (POST/DELETE /interactions, hydrated from the post's
// liked_by_me/like_count) and share (copy link / Web Share) — and clearly marks
// the ones that don't exist yet: Tip is Phase-1b (disabled), Save is a local-only
// toggle, and the music disc is decorative. The like count is the real journal
// aggregate; the other rail items keep action labels (no fabricated numbers).

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";

import type { Post } from "@contract/Post";

import { FEATURES } from "@/lib/features";
import { useLike } from "@/lib/useLike";
import {
  HeartIcon,
  CommentIcon,
  CoinIcon,
  ShareIcon,
  BookmarkIcon,
  MusicIcon,
} from "./icons";
import styles from "./reels.module.css";

// Compact German count for the rail label (1.234 → "1234", 12500 → "12,5 Tsd.").
function fmtCount(n: number): string {
  return new Intl.NumberFormat("de-DE", { notation: "compact" }).format(n);
}

export type LikeState = { liked: boolean; count: number };

export function ActionRail({
  post,
  token,
  likeState,
  onLikeChange,
}: {
  post: Post;
  token: string;
  /// Overrides the hydration source. The rail is keyed per reel and UNMOUNTS on
  /// every page — without this, scrolling back would re-hydrate from the
  /// fetch-time feed snapshot and visually revert a like made moments ago. The
  /// parent (ReelsFeed) keeps the post-toggle truth across mounts and feeds it
  /// back here; standalone usages can omit both props.
  likeState?: LikeState;
  onLikeChange?: (postId: string, state: LikeState) => void;
}) {
  const router = useRouter();
  const postId = String(post.id);
  const { liked, count, toggle } = useLike(
    { postId },
    token,
    likeState ?? { liked: post.liked_by_me, count: Number(post.like_count) },
  );
  useEffect(() => {
    onLikeChange?.(postId, { liked, count });
  }, [onLikeChange, postId, liked, count]);
  const [saved, setSaved] = useState(false);
  const [shareLabel, setShareLabel] = useState("Teilen");

  async function share() {
    const url = `${window.location.origin}/posts/${postId}`;
    const nav = navigator as Navigator & { share?: (d: { url: string; title?: string }) => Promise<void> };
    if (nav.share) {
      try {
        await nav.share({ url, title: "Poolsite" });
        return;
      } catch {
        /* user cancelled — fall through to copy */
      }
    }
    try {
      await navigator.clipboard.writeText(url);
      setShareLabel("Kopiert!");
      window.setTimeout(() => setShareLabel("Teilen"), 1500);
    } catch {
      /* clipboard blocked — no-op */
    }
  }

  return (
    <div className={styles.rail}>
      <div className={styles.railItem}>
        <button
          type="button"
          className={`${styles.glass} ${styles.circleBtn} ${liked ? styles.liked : ""}`}
          onClick={toggle}
          aria-pressed={liked}
          aria-label={liked ? "Gefällt dir nicht mehr" : "Gefällt mir"}
        >
          <HeartIcon size={26} filled={liked} />
        </button>
        <span className={styles.railLabel}>{fmtCount(count)}</span>
      </div>

      <div className={styles.railItem}>
        <button
          type="button"
          className={`${styles.glass} ${styles.circleBtn}`}
          onClick={() => router.push(`/posts/${postId}`)}
          aria-label="Kommentare öffnen"
        >
          <CommentIcon size={26} />
        </button>
        <span className={styles.railLabel}>Kommentar</span>
      </div>

      {FEATURES.tips && (
        <div className={styles.railItem}>
          <button
            type="button"
            className={`${styles.glass} ${styles.circleBtn}`}
            disabled
            title="Trinkgeld — bald verfügbar (Phase 1b)"
            aria-label="Trinkgeld (bald verfügbar)"
          >
            <CoinIcon size={26} />
          </button>
          <span className={styles.railLabel}>Tip</span>
        </div>
      )}

      <div className={styles.railItem}>
        <button
          type="button"
          className={`${styles.glass} ${styles.circleBtn}`}
          onClick={share}
          aria-label="Teilen"
        >
          <ShareIcon size={26} />
        </button>
        <span className={styles.railLabel}>{shareLabel}</span>
      </div>

      {FEATURES.saves && (
        <div className={styles.railItem}>
          <button
            type="button"
            className={`${styles.glass} ${styles.circleBtn} ${saved ? styles.saved : ""}`}
            onClick={() => setSaved((s) => !s)}
            aria-pressed={saved}
            aria-label={saved ? "Gespeichert" : "Speichern"}
            title="Nur lokal — serverseitiges Speichern folgt"
          >
            <BookmarkIcon size={26} filled={saved} />
          </button>
        </div>
      )}

      <div className={styles.railItem}>
        <span className={`${styles.glass} ${styles.circleBtn}`} aria-hidden="true">
          <MusicIcon size={24} />
        </span>
      </div>
    </div>
  );
}
