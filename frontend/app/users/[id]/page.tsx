"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";

import type { Follow } from "@contract/Follow";
import type { GemBalance } from "@contract/GemBalance";
import type { Post } from "@contract/Post";
import type { User } from "@contract/User";

import { apiFetch } from "@/lib/api";
import { PostCard } from "@/components/PostCard";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function ProfilePage() {
  const { token, userId, ready } = useRequireAuth();
  const params = useParams<{ id: string }>();
  const profileId = params.id;
  const isSelf = userId === profileId;

  const [user, setUser] = useState<User | null>(null);
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [followingCount, setFollowingCount] = useState<number | null>(null);
  const [balance, setBalance] = useState<GemBalance | null>(null);
  const [following, setFollowing] = useState<boolean | null>(null);
  // Distinguishes "still loading the viewer's follow list" (following === null,
  // followErr === null) from "the follow list failed" (followErr set) so we can
  // offer a retry instead of silently hiding the Follow button forever.
  const [followErr, setFollowErr] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Bumped to re-run only the viewer's follow-list fetch on "Retry".
  const [followReload, setFollowReload] = useState(0);

  useEffect(() => {
    if (!token || !profileId) return;
    // Reset all per-profile state before fetching so stale data/error from a
    // previously-viewed profile never flashes alongside the new one.
    setUser(null);
    setPosts(null);
    setFollowingCount(null);
    setBalance(null);
    setFollowing(null);
    setFollowErr(false);
    setError(null);
    // Guard against out-of-order resolution: a slow response for a previously-
    // viewed profile must not overwrite the profile we navigated to.
    let stale = false;
    apiFetch<User>(`/users/${profileId}`, { token })
      .then((u) => !stale && setUser(u))
      .catch(() => !stale && setError("Could not load this profile."));
    apiFetch<Post[]>(`/posts?author_id=${profileId}&limit=50`, { token })
      .then((p) => !stale && setPosts(p))
      .catch(() => !stale && setPosts([]));
    apiFetch<Follow[]>(`/users/${profileId}/following`, { token })
      .then((f) => !stale && setFollowingCount(f.length))
      .catch(() => {});
    if (isSelf) {
      apiFetch<GemBalance>(`/users/${profileId}/gems`, { token })
        .then((b) => !stale && setBalance(b))
        .catch(() => {});
    } else if (userId) {
      apiFetch<Follow[]>(`/users/${userId}/following`, { token })
        .then((f) => !stale && setFollowing(f.some((x) => String(x.followee_id) === profileId)))
        .catch(() => !stale && setFollowErr(true));
    }
    return () => {
      stale = true;
    };
  }, [token, userId, profileId, isSelf, followReload]);

  async function toggleFollow() {
    if (following === null || !token) return;
    const next = !following;
    setFollowing(next); // optimistic
    try {
      await apiFetch<void>(`/me/following/${profileId}`, {
        method: next ? "PUT" : "DELETE",
        token,
      });
    } catch {
      setFollowing(!next); // revert on failure
    }
  }

  if (!ready || !token) return null;

  return (
    <div>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {user && (
        <>
          <h1>
            user {String(user.id)} {isSelf && <small>(you)</small>}
          </h1>
          <p style={{ color: "#666" }}>
            Joined {new Date(user.created_at).toLocaleDateString()}
            {followingCount !== null && <> · following {followingCount}</>}
            {isSelf && balance && <> · {String(balance.balance)} gems</>}
          </p>
          {user.declared_categories.length > 0 && (
            <p>Interests: {user.declared_categories.join(", ")}</p>
          )}
          {!isSelf && following !== null && (
            <button type="button" onClick={toggleFollow}>
              {following ? "Unfollow" : "Follow"}
            </button>
          )}
          {!isSelf && followErr && (
            <p style={{ color: "crimson" }}>
              Couldn&apos;t check your follow status.{" "}
              <button type="button" onClick={() => setFollowReload((n) => n + 1)}>
                Retry
              </button>
            </p>
          )}
          {isSelf && (
            <p>
              <Link href="/compose">Write a post →</Link>
            </p>
          )}
          <h2>Posts</h2>
          {posts === null && <p>Loading…</p>}
          {posts !== null && posts.length === 0 && <p>No posts yet.</p>}
          {posts?.map((p) => (
            <PostCard key={String(p.id)} post={p} />
          ))}
        </>
      )}
    </div>
  );
}
