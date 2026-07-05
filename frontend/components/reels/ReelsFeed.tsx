"use client";

// The reels feed (Figma "Glass · Reels"), wired to the live API. A FADE PAGER:
// only the active reel's media/text layer changes — it crossfades — while the
// chrome (tabs, caption, action rail, bottom nav) stays pinned in place and
// updates to the active post. Paging is via the chevrons, the mouse wheel, a
// vertical swipe, or the arrow keys. "Für dich" is the cold-start feed; "Folge
// ich" filters it to the accounts the viewer follows.

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

import type { Follow } from "@contract/Follow";

import { apiFetch } from "@/lib/api";
import { usePagedFeed } from "@/lib/usePagedFeed";
import { ActionRail } from "./ActionRail";
import { ReelMedia } from "./ReelMedia";
import {
  ChevronUpIcon,
  ChevronDownIcon,
  HomeIcon,
  SearchIcon,
  CreateIcon,
  MessageIcon,
  ProfileIcon,
} from "./icons";
import styles from "./reels.module.css";

type Tab = "foryou" | "following";

// Compact German relative time. The platform has no username field yet, so the
// handle is derived from the author id (a real @handle needs a users.username col).
function relTime(iso: string): string {
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 60) return "gerade eben";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `vor ${mins} Min`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `vor ${hours} Std`;
  const days = Math.floor(hours / 24);
  return `vor ${days} ${days === 1 ? "Tag" : "Tagen"}`;
}

export function ReelsFeed({ token, userId }: { token: string; userId: string }) {
  const router = useRouter();
  // Cursor-paged feed (D1/D2): pages of 20, more loaded as the viewer nears
  // the end of the track.
  const { posts, error, loadMore, reload } = usePagedFeed(userId, token);
  const [tab, setTab] = useState<Tab>("foryou");
  const [followed, setFollowed] = useState<Set<string> | null>(null);
  const [activeIdx, setActiveIdx] = useState(0);

  const cooldown = useRef(false); // debounces one wheel/swipe gesture to one page
  const touchStartY = useRef<number | null>(null);

  useEffect(() => {
    let stale = false;
    apiFetch<Follow[]>(`/users/${userId}/following`, { token })
      .then((f) => !stale && setFollowed(new Set(f.map((x) => String(x.followee_id)))))
      .catch(() => !stale && setFollowed(new Set()));
    return () => {
      stale = true;
    };
  }, [userId, token]);

  const visible = useMemo(() => {
    if (!posts) return posts;
    if (tab === "following") {
      const set = followed ?? new Set<string>();
      return posts.filter((p) => set.has(String(p.author_id)));
    }
    return posts;
  }, [posts, tab, followed]);

  const count = visible?.length ?? 0;

  const page = useCallback(
    (dir: 1 | -1) => {
      setActiveIdx((i) => Math.min(Math.max(i + dir, 0), Math.max(count - 1, 0)));
    },
    [count],
  );

  // Debounced paging so one wheel/swipe gesture advances exactly one reel.
  const pageDebounced = useCallback(
    (dir: 1 | -1) => {
      if (cooldown.current) return;
      cooldown.current = true;
      page(dir);
      window.setTimeout(() => {
        cooldown.current = false;
      }, 480);
    },
    [page],
  );

  // Arrow keys / space page through the feed.
  useEffect(() => {
    if (!count) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "ArrowDown" || e.key === "PageDown" || e.key === " ") {
        e.preventDefault();
        pageDebounced(1);
      } else if (e.key === "ArrowUp" || e.key === "PageUp") {
        e.preventDefault();
        pageDebounced(-1);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [count, pageDebounced]);

  function selectTab(next: Tab) {
    setTab(next);
    setActiveIdx(0);
  }

  const idx = count ? Math.min(activeIdx, count - 1) : 0;
  const active = visible && count > 0 ? visible[idx] : null;

  // Prefetch: nearing the end of the visible track pulls the next page. A
  // no-op once the cursor is exhausted (the hook is single-flight), so firing
  // on every index change is safe. On the "Folge ich" tab this keeps fetching
  // pages to filter client-side — the server-side following feed is the real
  // fix and stays on the 1b list.
  useEffect(() => {
    if (count > 0 && idx >= count - 3) loadMore();
  }, [idx, count, loadMore]);

  function onWheel(e: React.WheelEvent) {
    if (Math.abs(e.deltaY) < 12) return;
    pageDebounced(e.deltaY > 0 ? 1 : -1);
  }
  function onTouchStart(e: React.TouchEvent) {
    touchStartY.current = e.touches[0]?.clientY ?? null;
  }
  function onTouchEnd(e: React.TouchEvent) {
    const start = touchStartY.current;
    if (start == null) return;
    const dy = start - (e.changedTouches[0]?.clientY ?? start);
    if (Math.abs(dy) > 44) pageDebounced(dy > 0 ? 1 : -1);
    touchStartY.current = null;
  }

  function stateBlock(children: ReactNode) {
    return <div className={styles.center}>{children}</div>;
  }

  let mediaContent: ReactNode;
  if (error) {
    mediaContent = stateBlock(
      <>
        <p>{error}</p>
        <button type="button" className={styles.ghostLink} onClick={reload}>
          Neu laden
        </button>
      </>,
    );
  } else if (visible == null) {
    mediaContent = stateBlock(<p>Lädt…</p>);
  } else if (visible.length === 0) {
    mediaContent = stateBlock(
      tab === "following" ? (
        <>
          <p>Noch nichts von Leuten, denen du folgst.</p>
          <button type="button" className={styles.ghostLink} onClick={() => selectTab("foryou")}>
            Zu „Für dich“
          </button>
        </>
      ) : (
        <>
          <p>Dein Feed ist leer.</p>
          <button type="button" className={styles.ghostLink} onClick={() => router.push("/compose")}>
            Ersten Post schreiben →
          </button>
        </>
      ),
    );
  } else {
    // The sliding track: one full-height slide per reel, translated by the active
    // index. ONLY this moves; the rail/tabs/caption/nav are pinned outside it. Media
    // is fetched/played only for the active slide.
    mediaContent = (
      <div className={styles.track} style={{ transform: `translateY(-${idx * 100}%)` }}>
        {visible.map((post, i) => (
          <div key={String(post.id)} className={styles.slide}>
            <ReelMedia
              mediaId={post.media_id != null ? String(post.media_id) : null}
              body={post.body}
              token={token}
              active={i === idx}
            />
          </div>
        ))}
      </div>
    );
  }

  return (
    <div className={styles.stage}>
      <div
        className={styles.card}
        onWheel={onWheel}
        onTouchStart={onTouchStart}
        onTouchEnd={onTouchEnd}
      >
        {mediaContent}

        <div className={styles.topFade} />
        <div className={styles.bottomFade} />

        <div className={styles.tabs} role="tablist" aria-label="Feed">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "following"}
            className={`${styles.tab} ${tab === "following" ? styles.tabActive : ""}`}
            onClick={() => selectTab("following")}
          >
            Folge ich
            {tab === "following" && <span className={styles.tabUnderline} />}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "foryou"}
            className={`${styles.tab} ${tab === "foryou" ? styles.tabActive : ""}`}
            onClick={() => selectTab("foryou")}
          >
            Für dich
            {tab === "foryou" && <span className={styles.tabUnderline} />}
          </button>
        </div>

        {/* Pinned caption — reflects the active reel, doesn't scroll away. */}
        {active && (
          <div className={styles.caption}>
            <div className={styles.handleRow}>
              <Link href={`/users/${String(active.author_id)}`} className={styles.handle}>
                @user-{String(active.author_id)}
              </Link>
            </div>
            {active.body && active.media_id != null && (
              <p className={styles.captionBody}>{active.body}</p>
            )}
            <div className={styles.metaRow}>
              <span className={styles.metaText}>
                {[active.category, relTime(active.created_at)].filter(Boolean).join(" · ")}
              </span>
            </div>
          </div>
        )}

        {/* Pinned action rail — keyed by post so its like state resets per reel. */}
        {active && <ActionRail key={String(active.id)} post={active} token={token} />}
      </div>

      {count > 1 && (
        <div className={styles.chevrons}>
          <button
            type="button"
            className={`${styles.glass} ${styles.chevBtn}`}
            onClick={() => page(-1)}
            disabled={activeIdx === 0}
            aria-label="Vorheriges"
          >
            <ChevronUpIcon size={22} />
          </button>
          <button
            type="button"
            className={`${styles.glass} ${styles.chevBtn}`}
            onClick={() => page(1)}
            disabled={activeIdx >= count - 1}
            aria-label="Nächstes"
          >
            <ChevronDownIcon size={22} />
          </button>
        </div>
      )}

      <nav className={`${styles.glass} ${styles.bottomNav}`} aria-label="Hauptnavigation">
        <button
          type="button"
          className={`${styles.navBtn} ${styles.navActive}`}
          aria-label="Home"
          onClick={() => setActiveIdx(0)}
        >
          <HomeIcon size={28} filled />
        </button>
        <button type="button" className={styles.navBtn} aria-label="Suche (bald)" disabled title="Bald verfügbar">
          <SearchIcon size={28} />
        </button>
        <button
          type="button"
          className={styles.navBtn}
          aria-label="Erstellen"
          onClick={() => router.push("/compose")}
        >
          <CreateIcon size={28} />
        </button>
        <button type="button" className={styles.navBtn} aria-label="Nachrichten (bald)" disabled title="Bald verfügbar">
          <MessageIcon size={28} />
        </button>
        <button
          type="button"
          className={styles.navBtn}
          aria-label="Profil"
          onClick={() => router.push(`/users/${userId}`)}
        >
          <ProfileIcon size={28} />
        </button>
      </nav>
    </div>
  );
}
