"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";

import type { Comment } from "@contract/Comment";
import type { NewComment } from "@contract/NewComment";
import type { NewInteraction } from "@contract/NewInteraction";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { useLike } from "@/lib/useLike";
import type { Wire } from "@/lib/wire";

// The per-comment like toggle, hydrated from the fetched row. Its own component
// so each comment gets its own hook state.
function CommentLikeButton({ comment, token }: { comment: Comment; token: string }) {
  const { liked, count, toggle } = useLike({ commentId: String(comment.id) }, token, {
    liked: comment.liked_by_me,
    count: Number(comment.like_count),
  });
  return (
    <button
      type="button"
      onClick={toggle}
      aria-pressed={liked}
      aria-label={liked ? "Gefällt dir nicht mehr" : "Gefällt mir"}
      style={{
        background: "none",
        border: 0,
        padding: 0,
        cursor: "pointer",
        color: liked ? "crimson" : "#888",
        fontSize: "0.78rem",
      }}
    >
      {liked ? "♥" : "♡"} {count.toLocaleString("de-DE")}
    </button>
  );
}

export function Comments({ postId, token }: { postId: string; token: string }) {
  const {
    data: comments,
    error: loadError,
    reload,
  } = useFetch<Comment[]>(() => apiFetch(`/posts/${postId}/comments`, { token }), [postId, token]);
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!body.trim()) return;
    setBusy(true);
    setSubmitError(null);
    try {
      const newComment: NewComment = { body: body.trim() };
      await apiFetch<Comment>(`/posts/${postId}/comments`, {
        method: "POST",
        body: newComment,
        token,
      });
      // Fire the comment interaction telemetry alongside the write (best-effort).
      // post_id is bigint in the contract but a number on the wire; Wire<> keeps
      // the field contract intact instead of erasing it with `as unknown as`.
      const interaction: Wire<NewInteraction> = {
        type: "comment",
        target_id: null,
        post_id: Number(postId),
        comment_id: null,
      };
      apiFetch<void>("/interactions", { method: "POST", body: interaction, token }).catch(() => {});
      setBody("");
      reload();
    } catch {
      setSubmitError("Kommentar konnte nicht gesendet werden.");
    } finally {
      setBusy(false);
    }
  }

  const error = loadError ? "Kommentare konnten nicht geladen werden." : submitError;

  return (
    <section style={{ marginTop: "1.5rem" }}>
      <h2 style={{ fontSize: "1.1rem" }}>Kommentare</h2>
      <form onSubmit={submit} style={{ display: "flex", gap: "0.5rem", marginBottom: "1rem" }}>
        <input
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="Kommentieren…"
          style={{ flex: 1 }}
        />
        <button type="submit" disabled={busy || !body.trim()}>
          Senden
        </button>
      </form>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {comments === null && <p>Lädt…</p>}
      {comments !== null && comments.length === 0 && (
        <p style={{ color: "#888" }}>Noch keine Kommentare.</p>
      )}
      {comments?.map((c) => (
        <div key={String(c.id)} style={{ borderTop: "1px solid #eee", padding: "0.5rem 0" }}>
          <div style={{ fontSize: "0.78rem", color: "#888" }}>
            <Link href={`/users/${c.author_id}`}>@user-{String(c.author_id)}</Link>
            {" · "}
            {new Date(c.created_at).toLocaleString()}
          </div>
          <p style={{ margin: "0.2rem 0", whiteSpace: "pre-wrap" }}>{c.body}</p>
          <CommentLikeButton comment={c} token={token} />
        </div>
      ))}
    </section>
  );
}
