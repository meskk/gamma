"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";

import type { Follow } from "@contract/Follow";
import type { GemBalance } from "@contract/GemBalance";
import type { Post } from "@contract/Post";
import type { User } from "@contract/User";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { PostCard } from "@/components/PostCard";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function ProfilePage() {
  const { token, userId, ready } = useRequireAuth();
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
  const followingCount = followingList ? followingList.length : null;
  const following =
    followOverride ??
    (viewerFollows ? viewerFollows.some((x) => String(x.followee_id) === profileId) : null);
  const followErr = !!followError;
  const error = userError ? "Profil konnte nicht geladen werden." : null;

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
    <div>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {user && (
        <>
          <h1>
            @user-{String(user.id)} {isSelf && <small>(du)</small>}
          </h1>
          <p style={{ color: "#666" }}>
            Dabei seit {new Date(user.created_at).toLocaleDateString()}
            {followingCount !== null && <> · folgt {followingCount}</>}
            {isSelf && balance && <> · {String(balance.balance)} Gems</>}
          </p>
          {user.declared_categories.length > 0 && (
            <p>Interessen: {user.declared_categories.join(", ")}</p>
          )}
          {!isSelf && following !== null && (
            <button type="button" onClick={toggleFollow}>
              {following ? "Entfolgen" : "Folgen"}
            </button>
          )}
          {!isSelf && followErr && (
            <p style={{ color: "crimson" }}>
              Follow-Status konnte nicht geprüft werden.{" "}
              <button type="button" onClick={retryFollows}>
                Erneut versuchen
              </button>
            </p>
          )}
          {isSelf && (
            <p>
              <Link href="/compose">Post schreiben →</Link>
            </p>
          )}
          <h2>Posts</h2>
          {posts === null && <p>Lädt…</p>}
          {posts !== null && posts.length === 0 && <p>Noch keine Posts.</p>}
          {posts?.map((p) => (
            <PostCard key={String(p.id)} post={p} />
          ))}
        </>
      )}
    </div>
  );
}
