"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";

import type { NewInteraction } from "@contract/NewInteraction";
import type { Post } from "@contract/Post";
import type { ReportRequest } from "@contract/ReportRequest";

import { ApiError, apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import type { Wire } from "@/lib/wire";
import { Comments } from "@/components/Comments";
import { MediaView } from "@/components/MediaView";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function PostDetailPage() {
  const { token, ready } = useRequireAuth();
  const params = useParams<{ id: string }>();
  const id = params.id;

  // Note (UI-lie limitation): the `Post` contract carries no per-viewer "liked"
  // flag, so we can't hydrate `liked` from the server — after a reload a post the
  // viewer already liked shows "♡ Like" until they interact. Backend fix required
  // (add a `liked_by_me` field to `Post`); until then this is a known cosmetic gap.
  const { data: post, error: loadError } = useFetch<Post>(
    () => apiFetch(`/posts/${id}`, { token }),
    [token, id],
    { enabled: !!token && !!id },
  );
  const [liked, setLiked] = useState(false);
  const [reported, setReported] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Per-post interaction state resets on navigation (the fetch state resets
  // inside useFetch) — a stale `liked` would mislabel the button and make
  // like() early-return on the NEW post.
  useEffect(() => {
    setLiked(false);
    setReported(false);
    setActionError(null);
  }, [id]);

  const error = loadError
    ? loadError instanceof ApiError && loadError.status === 404
      ? "This post doesn't exist (or was taken down)."
      : "Could not load this post."
    : actionError;

  async function like() {
    if (liked || !token) return;
    setLiked(true); // optimistic; the backend dedups likes, so a repeat is a no-op
    // ids are bigint in the contract but go on the wire as numbers (JSON can't
    // serialize bigint), so build with a number against Wire<NewInteraction> —
    // that still typechecks structurally, so a renamed/added field is caught.
    const body: Wire<NewInteraction> = {
      type: "like",
      target_id: null,
      post_id: Number(id),
    };
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
      setActionError("Could not submit the report.");
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
