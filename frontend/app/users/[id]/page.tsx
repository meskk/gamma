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
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!token || !profileId) return;
    apiFetch<User>(`/users/${profileId}`, { token })
      .then(setUser)
      .catch(() => setError("Could not load this profile."));
    apiFetch<Post[]>(`/posts?author_id=${profileId}&limit=50`, { token })
      .then(setPosts)
      .catch(() => setPosts([]));
    apiFetch<Follow[]>(`/users/${profileId}/following`, { token })
      .then((f) => setFollowingCount(f.length))
      .catch(() => {});
    if (isSelf) {
      apiFetch<GemBalance>(`/users/${profileId}/gems`, { token })
        .then(setBalance)
        .catch(() => {});
    } else if (userId) {
      apiFetch<Follow[]>(`/users/${userId}/following`, { token })
        .then((f) => setFollowing(f.some((x) => String(x.followee_id) === profileId)))
        .catch(() => {});
    }
  }, [token, userId, profileId, isSelf]);

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
