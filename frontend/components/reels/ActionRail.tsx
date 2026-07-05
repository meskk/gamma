"use client";

// The right-hand action rail. Wires the actions the backend actually supports —
// like (POST /interactions) and share (copy link / Web Share) — and clearly marks
// the ones that don't exist yet: Tip is Phase-1b (disabled), Save is a local-only
// toggle, and the music disc is decorative. Counts are shown only when a real value
// is provided (no fabricated numbers); otherwise a short action label is shown.

import { useState } from "react";
import { useRouter } from "next/navigation";

import type { Post } from "@contract/Post";
import type { NewInteraction } from "@contract/NewInteraction";

import { apiFetch } from "@/lib/api";
import {
  HeartIcon,
  CommentIcon,
  CoinIcon,
  ShareIcon,
  BookmarkIcon,
  MusicIcon,
} from "./icons";
import styles from "./reels.module.css";

export function ActionRail({ post, token }: { post: Post; token: string }) {
  const router = useRouter();
  const postId = String(post.id);
  const [liked, setLiked] = useState(false);
  const [saved, setSaved] = useState(false);
  const [shareLabel, setShareLabel] = useState("Teilen");

  async function like() {
    if (liked) return; // the backend records a like once; there is no un-like yet
    setLiked(true); // optimistic
    const body: NewInteraction = { type: "like", target_id: null, post_id: post.id };
    try {
      await apiFetch<unknown>("/interactions", { method: "POST", body, token });
    } catch {
      setLiked(false); // revert on failure
    }
  }

  async function share() {
    const url = `${window.location.origin}/posts/${postId}`;
    const nav = navigator as Navigator & { share?: (d: { url: string; title?: string }) => Promise<void> };
    if (nav.share) {
      try {
        await nav.share({ url, title: "Peer Network" });
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
          onClick={like}
          aria-pressed={liked}
          aria-label={liked ? "Gefällt dir" : "Gefällt mir"}
        >
          <HeartIcon size={26} filled={liked} />
        </button>
        <span className={styles.railLabel}>{liked ? "Geliked" : "Like"}</span>
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

      <div className={styles.railItem}>
        <span className={`${styles.glass} ${styles.circleBtn}`} aria-hidden="true">
          <MusicIcon size={24} />
        </span>
      </div>
    </div>
  );
}
