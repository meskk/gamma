"use client";

// The full-bleed background of a reel: the post's media (image/video/audio) when
// present and entitled, a paywall when the media is locked, or the post's text as a
// centred card when there's no media. Media is fetched lazily the first time the
// reel is near the viewport, and video only plays while the reel is on screen.

import { useEffect, useRef, useState } from "react";

import { useUnlock } from "@/lib/useUnlock";
import { ImageIcon, LockIcon } from "./icons";
import styles from "./reels.module.css";

type Props = {
  mediaId: string | null;
  body: string | null;
  token: string;
  active: boolean;
};

export function ReelMedia({ mediaId, body, token, active }: Props) {
  // Lazy-fetch the media the first time this reel becomes the active/visible
  // one — mapped onto the shared unlock flow's `enabled` flag.
  const [requested, setRequested] = useState(false);
  useEffect(() => {
    if (mediaId && active && !requested) setRequested(true);
  }, [mediaId, active, requested]);

  const { view, loadError, unlockError, unlocking, unlock, reload } = useUnlock(mediaId, token, {
    enabled: requested,
  });
  const videoRef = useRef<HTMLVideoElement | null>(null);

  // Only the on-screen reel's video plays; others pause (saves bandwidth/CPU).
  useEffect(() => {
    const v = videoRef.current;
    if (!v) return;
    if (active) void v.play().catch(() => {});
    else v.pause();
  }, [active, view]);

  // No media → show the post body as a centred text card (or the image glyph).
  if (!mediaId) {
    return (
      <div className={styles.placeholder}>
        {body ? (
          <p className={styles.textBody}>{body}</p>
        ) : (
          <span className={styles.placeholderIcon}>
            <ImageIcon size={72} />
          </span>
        )}
      </div>
    );
  }

  if (loadError) {
    return (
      <div className={styles.center}>
        <p>Medien konnten nicht geladen werden.</p>
        <button type="button" className={styles.ghostLink} onClick={reload}>
          Erneut versuchen
        </button>
      </div>
    );
  }

  if (!view) {
    return (
      <div className={styles.placeholder}>
        <span className={styles.placeholderIcon}>
          <ImageIcon size={72} />
        </span>
      </div>
    );
  }

  // Entitled + ready → play. (Video is the presigned raw URL; adaptive HLS via the
  // /media/:id/manifest endpoint is a follow-up once an HLS player is wired in.)
  if (view.playback_url) {
    const url = view.playback_url;
    if (view.kind === "video") {
      return (
        <div className={styles.media}>
          <video
            ref={videoRef}
            className={styles.mediaEl}
            src={url}
            muted
            loop
            playsInline
            preload="metadata"
          />
        </div>
      );
    }
    if (view.kind === "audio") {
      return (
        <div className={styles.placeholder}>
          <div className={styles.audioCard}>
            {body ? <p className={styles.textBody}>{body}</p> : null}
            <audio src={url} controls loop />
          </div>
        </div>
      );
    }
    // image (default) — a presigned remote URL, not a bundled asset, so next/image
    // optimisation doesn't apply; a plain <img> is correct here.
    return (
      <div className={styles.media}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img className={styles.mediaEl} src={url} alt="" />
      </div>
    );
  }

  // Locked → paywall. An unlock failure keeps the paywall (retryable) and
  // shows its message beneath the button.
  if (Number(view.unlock_price) > 0) {
    return (
      <div className={styles.center}>
        <span className={styles.lockIcon}>
          <LockIcon size={40} />
        </span>
        <p>Dieses {view.kind === "video" ? "Video" : "Medium"} ist gesperrt.</p>
        <button type="button" className={styles.primaryBtn} onClick={unlock} disabled={unlocking}>
          {unlocking ? "Wird freigeschaltet…" : `Für ${String(view.unlock_price)} Gems freischalten`}
        </button>
        {unlockError && <p>Freischalten fehlgeschlagen — genug Gems?</p>}
      </div>
    );
  }

  // Free but still transcoding.
  return (
    <div className={styles.center}>
      <p>Medium wird noch verarbeitet…</p>
    </div>
  );
}
