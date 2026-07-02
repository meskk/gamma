"use client";

// One full-height reel slide: the media/text background, legibility gradients, the
// caption (author + body + meta) and the action rail.

import Link from "next/link";
import { forwardRef } from "react";

import type { Post } from "@contract/Post";

import { ActionRail } from "./ActionRail";
import { ReelMedia } from "./ReelMedia";
import styles from "./reels.module.css";

// Compact German relative time — the platform has no username field yet, so the
// handle is derived from the author id (a Phase-1a limitation; a real @handle needs
// a users.username column).
function relTime(iso: string): string {
  const then = new Date(iso).getTime();
  const secs = Math.max(0, (Date.now() - then) / 1000);
  if (secs < 60) return "gerade eben";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `vor ${mins} Min`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `vor ${hours} Std`;
  const days = Math.floor(hours / 24);
  return `vor ${days} ${days === 1 ? "Tag" : "Tagen"}`;
}

type Props = { post: Post; token: string; active: boolean };

export const Reel = forwardRef<HTMLDivElement, Props>(function Reel({ post, token, active }, ref) {
  const authorId = String(post.author_id);
  const meta = [post.category, relTime(post.created_at)].filter(Boolean).join(" · ");

  return (
    <section ref={ref} className={styles.reel} data-active={active}>
      <ReelMedia mediaId={post.media_id != null ? String(post.media_id) : null} body={post.body} token={token} active={active} />

      <div className={styles.topFade} />
      <div className={styles.bottomFade} />

      <div className={styles.caption}>
        <div className={styles.handleRow}>
          <Link href={`/users/${authorId}`} className={styles.handle}>
            @user-{authorId}
          </Link>
        </div>
        {post.body && post.media_id != null && <p className={styles.captionBody}>{post.body}</p>}
        {meta && (
          <div className={styles.metaRow}>
            <span className={styles.metaText}>{meta}</span>
          </div>
        )}
      </div>

      <ActionRail post={post} token={token} />
    </section>
  );
});
