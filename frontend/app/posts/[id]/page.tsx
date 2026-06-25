"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";

import type { NewInteraction } from "@contract/NewInteraction";
import type { Post } from "@contract/Post";
import type { ReportRequest } from "@contract/ReportRequest";

import { ApiError, apiFetch } from "@/lib/api";
import { Comments } from "@/components/Comments";
import { MediaView } from "@/components/MediaView";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function PostDetailPage() {
  const { token, ready } = useRequireAuth();
  const params = useParams<{ id: string }>();
  const id = params.id;

  const [post, setPost] = useState<Post | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [liked, setLiked] = useState(false);
  const [reported, setReported] = useState(false);

  useEffect(() => {
    if (!token || !id) return;
    // Reset per-post state before fetching so navigating between posts never
    // carries over stale data — in particular a stale `liked`/`reported` from the
    // previous post (which would mislabel the buttons and make like() early-return).
    setPost(null);
    setError(null);
    setLiked(false);
    setReported(false);
    apiFetch<Post>(`/posts/${id}`, { token })
      .then(setPost)
      .catch((e) =>
        setError(
          e instanceof ApiError && e.status === 404
            ? "This post doesn't exist (or was taken down)."
            : "Could not load this post.",
        ),
      );
  }, [token, id]);

  async function like() {
    if (liked || !token) return;
    setLiked(true); // optimistic; the backend dedups likes, so a repeat is a no-op
    // ids are bigint in the contract but go on the wire as numbers (JSON can't
    // serialize bigint), so build with a number and cast to the contract type.
    const body = {
      type: "like",
      target_id: null,
      post_id: Number(id),
    } as unknown as NewInteraction;
    try {
      await apiFetch<void>("/interactions", { method: "POST", body, token });
    } catch {
      setLiked(false); // revert on failure
    }
  }

  async function report() {
    if (reported || !token) return;
    const reason = window.prompt("Why are you reporting this post?");
    if (!reason) return;
    const body: ReportRequest = { reason };
    try {
      await apiFetch<void>(`/posts/${id}/report`, { method: "POST", body, token });
      setReported(true);
    } catch {
      setError("Could not submit the report.");
    }
  }

  if (!ready || !token) return null;

  return (
    <div>
      <p>
        <Link href="/feed">← Back to feed</Link>
      </p>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {!post && !error && <p>Loading…</p>}
      {post && (
        <article>
          <div style={{ fontSize: "0.8rem", color: "#888" }}>
            <Link href={`/users/${post.author_id}`}>user {String(post.author_id)}</Link>
            {post.category ? ` · ${post.category}` : ""}
            {" · "}
            {new Date(post.created_at).toLocaleString()}
          </div>
          <p style={{ fontSize: "1.1rem", whiteSpace: "pre-wrap", margin: "1rem 0" }}>
            {post.body ?? <em>(no text)</em>}
          </p>
          {post.media_id != null && (
            <div style={{ margin: "1rem 0" }}>
              <MediaView mediaId={String(post.media_id)} token={token} />
            </div>
          )}
          <div style={{ display: "flex", gap: "0.75rem" }}>
            <button type="button" onClick={like} disabled={liked}>
              {liked ? "♥ Liked" : "♡ Like"}
            </button>
            <button type="button" onClick={report} disabled={reported}>
              {reported ? "Reported" : "Report"}
            </button>
          </div>
          <Comments postId={id} token={token} />
        </article>
      )}
    </div>
  );
}
