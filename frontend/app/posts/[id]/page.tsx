"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";

import type { Post } from "@contract/Post";
import type { ReportRequest } from "@contract/ReportRequest";

import { ApiError, apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { useLike } from "@/lib/useLike";
import { Comments } from "@/components/Comments";
import { MediaView } from "@/components/MediaView";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function PostDetailPage() {
  const { token, ready } = useRequireAuth();
  const params = useParams<{ id: string }>();
  const id = params.id;

  const { data: post, error: loadError } = useFetch<Post>(
    () => apiFetch(`/posts/${id}`, { token }),
    [token, id],
    { enabled: !!token && !!id },
  );
  // Hydrated from the fetched post (liked_by_me/like_count), toggled optimistically.
  const { liked, count, toggle } = useLike(
    { postId: id },
    token ?? "",
    post ? { liked: post.liked_by_me, count: Number(post.like_count) } : null,
  );
  const [reported, setReported] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Per-post interaction state resets on navigation (fetch state resets inside
  // useFetch, liked inside useLike).
  useEffect(() => {
    setReported(false);
    setActionError(null);
  }, [id]);

  const error = loadError
    ? loadError instanceof ApiError && loadError.status === 404
      ? "Diesen Post gibt es nicht (oder er wurde entfernt)."
      : "Post konnte nicht geladen werden."
    : actionError;

  async function report() {
    if (reported || !token) return;
    const reason = window.prompt("Warum meldest du diesen Post?");
    if (!reason) return;
    const body: ReportRequest = { reason };
    try {
      await apiFetch<void>(`/posts/${id}/report`, { method: "POST", body, token });
      setReported(true);
    } catch {
      setActionError("Meldung konnte nicht gesendet werden.");
    }
  }

  if (!ready || !token) return null;

  return (
    <div>
      <p>
        <Link href="/feed">← Zurück zum Feed</Link>
      </p>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {!post && !error && <p>Lädt…</p>}
      {post && (
        <article>
          <div style={{ fontSize: "0.8rem", color: "#888" }}>
            <Link href={`/users/${post.author_id}`}>@user-{String(post.author_id)}</Link>
            {post.category ? ` · ${post.category}` : ""}
            {" · "}
            {new Date(post.created_at).toLocaleString()}
          </div>
          <p style={{ fontSize: "1.1rem", whiteSpace: "pre-wrap", margin: "1rem 0" }}>
            {post.body ?? <em>(kein Text)</em>}
          </p>
          {post.media_id != null && (
            <div style={{ margin: "1rem 0" }}>
              <MediaView mediaId={String(post.media_id)} token={token} />
            </div>
          )}
          <div style={{ display: "flex", gap: "0.75rem" }}>
            <button type="button" onClick={toggle} aria-pressed={liked}>
              {liked ? "♥ Gefällt dir" : "♡ Gefällt mir"}
              {` · ${count.toLocaleString("de-DE")}`}
            </button>
            <button type="button" onClick={report} disabled={reported}>
              {reported ? "Gemeldet" : "Melden"}
            </button>
          </div>
          <Comments postId={id} token={token} />
        </article>
      )}
    </div>
  );
}
