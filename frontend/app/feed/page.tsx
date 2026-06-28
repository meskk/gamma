"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

import type { GemBalance } from "@contract/GemBalance";
import type { Post } from "@contract/Post";

import { apiFetch } from "@/lib/api";
import { PostCard } from "@/components/PostCard";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function FeedPage() {
  const { token, userId, ready } = useRequireAuth();
  const [posts, setPosts] = useState<Post[] | null>(null);
  const [balance, setBalance] = useState<GemBalance | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!token || !userId) return;
    // Reset before (re)fetching so a previous error/feed doesn't linger — a transient
    // failure then self-heals on the next run instead of sticking until a full reload.
    setPosts(null);
    setBalance(null);
    setError(null);
    apiFetch<Post[]>(`/users/${userId}/feed?limit=50`, { token })
      .then(setPosts)
      .catch(() => setError("Could not load your feed."));
    apiFetch<GemBalance>(`/users/${userId}/gems`, { token })
      .then(setBalance)
      .catch(() => {
        /* balance is non-critical; ignore */
      });
  }, [token, userId]);

  if (!ready || !token) return null;

  return (
    <div>
      <div style={{ display: "flex", alignItems: "baseline", gap: "1rem" }}>
        <h1>Your feed</h1>
        {balance && (
          <span style={{ marginLeft: "auto", color: "#666" }}>{String(balance.balance)} gems</span>
        )}
      </div>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {posts === null && !error && <p>Loading…</p>}
      {posts !== null && posts.length === 0 && (
        <p>
          Your feed is empty. <Link href="/compose">Write the first post →</Link>
        </p>
      )}
      {posts?.map((p) => (
        <PostCard key={String(p.id)} post={p} />
      ))}
    </div>
  );
}
