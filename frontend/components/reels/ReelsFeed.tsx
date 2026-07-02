"use client";

// The reels feed: a vertical scroll-snap stack of posts with the "Folge ich / Für
// dich" tabs, desktop paging chevrons, and the bottom nav — the Figma "Glass ·
// Reels" screen, wired to the live API. "Für dich" is the cold-start feed; "Folge
// ich" filters it to the accounts the viewer follows.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useRouter } from "next/navigation";

import type { Post } from "@contract/Post";
import type { Follow } from "@contract/Follow";

import { apiFetch } from "@/lib/api";
import { Reel } from "./Reel";
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

export function ReelsFeed({ token, userId }: { token: string; userId: string }) {
  const router = useRouter();
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("foryou");
  const [followed, setFollowed] = useState<Set<string> | null>(null);
  const [activeIdx, setActiveIdx] = useState(0);

  const viewportRef = useRef<HTMLDivElement | null>(null);
  const reelRefs = useRef<Array<HTMLDivElement | null>>([]);

  // Load the feed.
  useEffect(() => {
    let stale = false;
    setPosts(null);
    setError(null);
    apiFetch<Post[]>(`/users/${userId}/feed?limit=50`, { token })
      .then((p) => !stale && setPosts(p))
      .catch(() => !stale && setError("Feed konnte nicht geladen werden."));
    return () => {
      stale = true;
    };
  }, [userId, token]);

  // Load who the viewer follows (drives the "Folge ich" tab). Non-critical.
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

  // Track the on-screen reel so only it plays and lazy-loads its media.
  useEffect(() => {
    const vp = viewportRef.current;
    if (!vp || !visible || visible.length === 0) return;
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting && e.intersectionRatio >= 0.6) {
            const idx = Number((e.target as HTMLElement).dataset.idx);
            if (!Number.isNaN(idx)) setActiveIdx(idx);
          }
        }
      },
      { root: vp, threshold: [0.6] },
    );
    reelRefs.current.forEach((el) => el && io.observe(el));
    return () => io.disconnect();
  }, [visible]);

  function selectTab(next: Tab) {
    setTab(next);
    setActiveIdx(0);
    viewportRef.current?.scrollTo({ top: 0 });
  }

  function page(dir: 1 | -1) {
    const count = visible?.length ?? 0;
    const next = Math.min(Math.max(activeIdx + dir, 0), Math.max(count - 1, 0));
    reelRefs.current[next]?.scrollIntoView({ behavior: "smooth" });
  }

  function stateBlock(children: ReactNode) {
    return (
      <section className={styles.reel}>
        <div className={styles.center}>{children}</div>
      </section>
    );
  }

  let content: ReactNode;
  if (error) {
    content = stateBlock(
      <>
        <p>{error}</p>
        <button type="button" className={styles.ghostLink} onClick={() => router.refresh()}>
          Neu laden
        </button>
      </>,
    );
  } else if (visible === null || visible === undefined) {
    content = stateBlock(<p>Lädt…</p>);
  } else if (visible.length === 0) {
    content = stateBlock(
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
    reelRefs.current = [];
    content = visible.map((post, i) => (
      <Reel
        key={String(post.id)}
        post={post}
        token={token}
        active={i === activeIdx}
        ref={(el) => {
          reelRefs.current[i] = el;
          if (el) el.dataset.idx = String(i);
        }}
      />
    ));
  }

  const hasMany = !!visible && visible.length > 1;

  return (
    <div className={styles.stage}>
      <div className={styles.card}>
        <div className={styles.viewport} ref={viewportRef}>
          {content}
        </div>

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
      </div>

      {hasMany && (
        <div className={styles.chevrons}>
          <button
            type="button"
            className={`${styles.glass} ${styles.chevBtn}`}
            onClick={() => page(-1)}
            aria-label="Vorheriges"
          >
            <ChevronUpIcon size={22} />
          </button>
          <button
            type="button"
            className={`${styles.glass} ${styles.chevBtn}`}
            onClick={() => page(1)}
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
          onClick={() => reelRefs.current[0]?.scrollIntoView({ behavior: "smooth" })}
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
