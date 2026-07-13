"use client";

// The user profile — Figma "Glass · Profile". A full-bleed dark screen with its
// own glass bottom nav (a BARE route, see AppShell.tsx), mirroring the "Glass ·
// Reels" feed. All the live data wiring (four stale-guarded reads + the
// optimistic follow toggle) is unchanged from the functional version; only the
// presentation is the glass redesign.
//
// The design shows fields the Phase-1a backend does not have yet (display name,
// bio, followers/likes aggregates, messaging). Per the owner's call we render
// only real data and OMIT the rest rather than fake it. "Subscribe" is shown
// only when the creator actually has a subscription private area configured
// (GAMMA_PRIVATE_AREA); the purchase flow itself is not built, so the pill is a
// display of the offer, not a checkout.

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState, type ReactNode } from "react";

import type { Follow } from "@contract/Follow";
import type { GemBalance } from "@contract/GemBalance";
import type { Post } from "@contract/Post";
import type { PrivateAreaView } from "@contract/PrivateAreaView";
import type { User } from "@contract/User";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { useRequireAuth } from "@/lib/useRequireAuth";
import {
  VerifiedIcon,
  LockIcon,
  ImageIcon,
  HomeIcon,
  SearchIcon,
  CreateIcon,
  MessageIcon,
  ProfileIcon,
} from "@/components/reels/icons";

const HANKEN = "var(--font-hanken), var(--font-inter), sans-serif";
const INTER = "var(--font-inter), system-ui, sans-serif";

// Shared glass surface (matches reels.module.css .glass).
const glass = {
  background: "rgba(255,255,255,0.16)",
  border: "1.2px solid rgba(255,255,255,0.4)",
  boxShadow: "0 8px 20px rgba(0,0,0,0.4)",
  backdropFilter: "blur(12px)",
  WebkitBackdropFilter: "blur(12px)",
} as const;

function fmtPrice(cents: number, currency: string): string {
  const amount = (cents / 100).toLocaleString("de-DE", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
  return currency === "EUR" ? `€${amount}` : `${amount} ${currency}`;
}

export default function ProfilePage() {
  const { token, userId, ready } = useRequireAuth();
  const router = useRouter();
  const params = useParams<{ id: string }>();
  const profileId = params.id;
  const isSelf = userId === profileId;
  const enabled = !!token && !!profileId;

  // Four independent reads, each stale-guarded and reset-on-navigation by
  // useFetch (this page used to hand-roll all of that in one big effect).
  const { data: user, error: userError } = useFetch<User>(
    () => apiFetch(`/users/${profileId}`, { token }),
    [token, profileId],
    { enabled },
  );
  const { data: postsData, error: postsError } = useFetch<Post[]>(
    () => apiFetch(`/posts?author_id=${profileId}&limit=50`, { token }),
    [token, profileId],
    { enabled },
  );
  const { data: followingList } = useFetch<Follow[]>(
    () => apiFetch(`/users/${profileId}/following`, { token }),
    [token, profileId],
    { enabled },
  );
  const { data: balance } = useFetch<GemBalance>(
    () => apiFetch(`/users/${profileId}/gems`, { token }),
    [token, profileId],
    { enabled: enabled && isSelf },
  );
  // The creator's private-area terms — the "Subscribe" offer. This route only
  // exists when GAMMA_PRIVATE_AREA is on; otherwise the fetch 404s and we treat
  // it as "no offer" (no pill). enabled only for other people's profiles.
  const { data: areaTerms } = useFetch<PrivateAreaView>(
    () => apiFetch(`/users/${profileId}/private-area`, { token }),
    [token, profileId],
    { enabled: enabled && !isSelf },
  );
  // The viewer's own follow list decides the Follow/Unfollow button; its
  // failure gets an explicit Retry (reload) instead of hiding the button.
  const {
    data: viewerFollows,
    error: followError,
    reload: retryFollows,
  } = useFetch<Follow[]>(
    () => apiFetch(`/users/${userId}/following`, { token }),
    [token, userId, profileId],
    { enabled: enabled && !isSelf && !!userId },
  );

  // Optimistic follow toggle: a local override on top of the server truth,
  // cleared when navigating to another profile.
  const [followOverride, setFollowOverride] = useState<boolean | null>(null);
  useEffect(() => setFollowOverride(null), [profileId]);

  const posts = postsData ?? (postsError ? [] : null);
  const postCount = posts ? posts.length : null;
  const followingCount = followingList ? followingList.length : null;
  const following =
    followOverride ??
    (viewerFollows ? viewerFollows.some((x) => String(x.followee_id) === profileId) : null);
  const followErr = !!followError;
  const error = userError ? "Profil konnte nicht geladen werden." : null;
  const subscribable =
    !!areaTerms && areaTerms.access_model === "subscription" && areaTerms.price_cents > 0;

  async function toggleFollow() {
    if (following === null || !token) return;
    const next = !following;
    setFollowOverride(next); // optimistic
    try {
      await apiFetch<void>(`/me/following/${profileId}`, {
        method: next ? "PUT" : "DELETE",
        token,
      });
    } catch {
      setFollowOverride(!next); // revert on failure
    }
  }

  if (!ready || !token) return null;

  return (
    <div
      style={{
        minHeight: "100dvh",
        background: "linear-gradient(180deg, #121214 0%, #080809 100%)",
        color: "#fff",
        fontFamily: INTER,
        paddingBottom: 120,
      }}
    >
      <div style={{ maxWidth: 1000, margin: "0 auto", padding: "72px 20px 0" }}>
        {error && (
          <p style={{ color: "#ff8a8a", textAlign: "center" }}>{error}</p>
        )}

        {user && (
          <>
            {/* ── Identity header ─────────────────────────────────────── */}
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
              {/* Avatar (placeholder — no avatar field yet) */}
              <div
                style={{
                  width: 112,
                  height: 112,
                  borderRadius: 999,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  border: "1.5px solid rgba(255,255,255,0.22)",
                  background: "linear-gradient(135deg, #3a3a40 0%, #17171a 50%)",
                  boxShadow: "0 10px 28px -2px rgba(0,0,0,0.5)",
                  color: "rgba(255,255,255,0.8)",
                }}
              >
                <ProfileIcon size={50} />
              </div>

              {/* Name (handle-derived) + verified */}
              <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 6 }}>
                <span style={{ fontFamily: HANKEN, fontWeight: 700, fontSize: 30, color: "#fff" }}>
                  @user-{String(user.id)}
                </span>
                {user.bot_gate_v && (
                  <span style={{ color: "#4aa8ff", display: "flex" }} title="Verifiziert">
                    <VerifiedIcon size={22} />
                  </span>
                )}
              </div>
              {isSelf && (
                <span style={{ fontSize: 13, color: "rgba(255,255,255,0.5)" }}>Das bist du</span>
              )}

              {/* Stats — only what the backend actually has. */}
              <div style={{ display: "flex", gap: 40, paddingTop: 8 }}>
                <Stat value={postCount} label="Posts" />
                <Stat value={followingCount} label="Folgt" />
                {isSelf && balance && (
                  <Stat value={Number(balance.balance)} label="Gems" />
                )}
              </div>

              {/* Actions */}
              <div style={{ display: "flex", gap: 10, paddingTop: 10, flexWrap: "wrap", justifyContent: "center" }}>
                {subscribable && (
                  <span
                    style={{ ...pill, background: "rgba(255,255,255,0.2)", cursor: "default" }}
                    title="Abo-Kauf folgt"
                  >
                    Abonnieren · {fmtPrice(Number(areaTerms!.price_cents), areaTerms!.currency)}
                  </span>
                )}
                {!isSelf && following !== null && (
                  <button
                    type="button"
                    onClick={toggleFollow}
                    style={{
                      ...pill,
                      background: following ? "rgba(255,255,255,0.12)" : "rgba(255,255,255,0.2)",
                      cursor: "pointer",
                    }}
                  >
                    {following ? "Entfolgen" : "Folgen"}
                  </button>
                )}
                {isSelf && (
                  <button
                    type="button"
                    onClick={() => router.push("/compose")}
                    style={{ ...pill, background: "rgba(255,255,255,0.2)", cursor: "pointer" }}
                  >
                    Post erstellen
                  </button>
                )}
              </div>

              {!isSelf && followErr && (
                <p style={{ color: "#ff8a8a", fontSize: 13 }}>
                  Follow-Status konnte nicht geprüft werden.{" "}
                  <button
                    type="button"
                    onClick={retryFollows}
                    style={{ background: "none", border: 0, color: "#4aa8ff", cursor: "pointer", padding: 0 }}
                  >
                    Erneut versuchen
                  </button>
                </p>
              )}
            </div>

            {/* ── Post grid ───────────────────────────────────────────── */}
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
                gap: 20,
                marginTop: 40,
              }}
            >
              {posts?.map((p, i) => (
                <PostTile key={String(p.id)} post={p} shade={i} />
              ))}
            </div>

            {posts !== null && posts.length === 0 && (
              <p style={{ textAlign: "center", color: "rgba(255,255,255,0.55)", marginTop: 48 }}>
                Noch keine Posts.
              </p>
            )}
            {posts === null && !error && (
              <p style={{ textAlign: "center", color: "rgba(255,255,255,0.55)", marginTop: 48 }}>
                Lädt…
              </p>
            )}
          </>
        )}
      </div>

      {/* ── Glass bottom nav (matches "Glass · Reels") ──────────────── */}
      <nav
        aria-label="Hauptnavigation"
        style={{
          ...glass,
          position: "fixed",
          left: "50%",
          bottom: 24,
          transform: "translateX(-50%)",
          display: "flex",
          gap: 32,
          alignItems: "center",
          padding: "16px 30px",
          borderRadius: 999,
        }}
      >
        <NavBtn label="Home" onClick={() => router.push("/feed")}>
          <HomeIcon size={28} />
        </NavBtn>
        <NavBtn label="Suche (bald)" disabled>
          <SearchIcon size={28} />
        </NavBtn>
        <NavBtn label="Erstellen" onClick={() => router.push("/compose")}>
          <CreateIcon size={28} />
        </NavBtn>
        <NavBtn label="Nachrichten (bald)" disabled>
          <MessageIcon size={28} />
        </NavBtn>
        <NavBtn label="Profil" active onClick={() => userId && router.push(`/users/${userId}`)}>
          <ProfileIcon size={28} filled />
        </NavBtn>
      </nav>
    </div>
  );
}

const pill = {
  display: "inline-flex",
  alignItems: "center",
  padding: "13px 24px",
  borderRadius: 999,
  border: "1.2px solid rgba(255,255,255,0.4)",
  boxShadow: "0 10px 26px rgba(0,0,0,0.45)",
  color: "#fff",
  fontFamily: INTER,
  fontWeight: 500,
  fontSize: 14,
} as const;

function Stat({ value, label }: { value: number | null; label: string }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 2 }}>
      <span style={{ fontFamily: HANKEN, fontWeight: 700, fontSize: 20, color: "#fff" }}>
        {value === null ? "–" : value.toLocaleString("de-DE")}
      </span>
      <span style={{ fontSize: 11, color: "rgba(255,255,255,0.45)" }}>{label}</span>
    </div>
  );
}

// A single post tile. Media posts show an image placeholder (no thumbnail fetch
// yet); text-only posts show a snippet. Private posts get a lock overlay.
function PostTile({ post, shade }: { post: Post; shade: number }) {
  const locked = post.area === "private";
  // Subtle per-tile gradient variation, like the Figma tiles.
  const top = 40 + (shade % 4) * 4;
  return (
    <Link
      href={`/posts/${post.id}`}
      style={{
        position: "relative",
        aspectRatio: "1 / 1",
        borderRadius: 20,
        overflow: "hidden",
        background: `linear-gradient(120deg, rgb(${top},${top},${top + 6}) 0%, rgb(22,22,25) 62%)`,
        boxShadow: "0 10px 26px -4px rgba(0,0,0,0.4)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        textDecoration: "none",
        color: "rgba(255,255,255,0.7)",
        padding: 16,
        boxSizing: "border-box",
      }}
    >
      {post.media_id != null || locked ? (
        <ImageIcon size={40} />
      ) : (
        <span
          style={{
            fontSize: 14,
            color: "rgba(255,255,255,0.75)",
            display: "-webkit-box",
            WebkitLineClamp: 5,
            WebkitBoxOrient: "vertical",
            overflow: "hidden",
            textAlign: "center",
          }}
        >
          {post.body ?? "(kein Text)"}
        </span>
      )}

      {locked && (
        <>
          <span style={{ position: "absolute", top: "38%", color: "rgba(255,255,255,0.85)" }}>
            <LockIcon size={22} />
          </span>
          <span
            style={{
              position: "absolute",
              top: "54%",
              padding: "9px 16px",
              borderRadius: 999,
              fontSize: 13,
              ...glass,
              background: "rgba(255,255,255,0.16)",
            }}
          >
            Privat
          </span>
        </>
      )}
    </Link>
  );
}

function NavBtn({
  children,
  label,
  onClick,
  disabled,
  active,
}: {
  children: ReactNode;
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  active?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={disabled ? "Bald verfügbar" : undefined}
      onClick={onClick}
      disabled={disabled}
      style={{
        background: "none",
        border: 0,
        padding: 0,
        display: "flex",
        cursor: disabled ? "default" : "pointer",
        color: active ? "#fff" : "rgba(255,255,255,0.82)",
        opacity: disabled ? 0.45 : 1,
      }}
    >
      {children}
    </button>
  );
}
