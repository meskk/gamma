"use client";

import Link from "next/link";
import { useCallback, useEffect, useState, type FormEvent } from "react";

import type { Comment } from "@contract/Comment";
import type { NewComment } from "@contract/NewComment";
import type { NewInteraction } from "@contract/NewInteraction";

import { apiFetch } from "@/lib/api";
import type { Wire } from "@/lib/wire";

export function Comments({ postId, token }: { postId: string; token: string }) {
  const [comments, setComments] = useState<Comment[] | null>(null);
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    apiFetch<Comment[]>(`/posts/${postId}/comments`, { token })
      .then(setComments)
      .catch(() => setError("Could not load comments."));
  }, [postId, token]);

  useEffect(() => {
    load();
  }, [load]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!body.trim()) return;
    setBusy(true);
    setError(null);
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
      };
      apiFetch<void>("/interactions", { method: "POST", body: interaction, token }).catch(() => {});
      setBody("");
      load();
    } catch {
      setError("Could not post your comment.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section style={{ marginTop: "1.5rem" }}>
      <h2 style={{ fontSize: "1.1rem" }}>Comments</h2>
      <form onSubmit={submit} style={{ display: "flex", gap: "0.5rem", marginBottom: "1rem" }}>
        <input
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="Add a comment…"
          style={{ flex: 1 }}
        />
        <button type="submit" disabled={busy || !body.trim()}>
          Post
        </button>
      </form>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {comments === null && <p>Loading…</p>}
      {comments !== null && comments.length === 0 && (
        <p style={{ color: "#888" }}>No comments yet.</p>
      )}
      {comments?.map((c) => (
        <div key={String(c.id)} style={{ borderTop: "1px solid #eee", padding: "0.5rem 0" }}>
          <div style={{ fontSize: "0.78rem", color: "#888" }}>
            <Link href={`/users/${c.author_id}`}>user {String(c.author_id)}</Link>
            {" · "}
            {new Date(c.created_at).toLocaleString()}
          </div>
          <p style={{ margin: "0.2rem 0", whiteSpace: "pre-wrap" }}>{c.body}</p>
        </div>
      ))}
    </section>
  );
}
