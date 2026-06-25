"use client";

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import type { NewPost } from "@contract/NewPost";
import type { Post } from "@contract/Post";

import { apiFetch } from "@/lib/api";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function ComposePage() {
  const { token, ready } = useRequireAuth();
  const router = useRouter();
  const [body, setBody] = useState("");
  const [category, setCategory] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!token) return;
    setBusy(true);
    setError(null);
    try {
      const payload: NewPost = {
        body: body.trim(),
        category: category.trim() ? category.trim() : null,
        media_id: null, // attaching media lands in Media part 2
      };
      const created = await apiFetch<Post>("/posts", { method: "POST", body: payload, token });
      router.push(`/posts/${created.id}`);
    } catch {
      setError("Could not publish — please try again.");
      setBusy(false);
    }
  }

  if (!ready || !token) return null;

  return (
    <div>
      <h1>New post</h1>
      <form onSubmit={onSubmit} style={{ display: "grid", gap: "0.75rem", maxWidth: 520 }}>
        <label>
          What&apos;s on your mind?
          <br />
          <textarea
            value={body}
            onChange={(e) => setBody(e.target.value)}
            required
            rows={5}
            style={{ width: "100%", fontFamily: "inherit" }}
          />
        </label>
        <label>
          Category <small>(optional)</small>
          <br />
          <input
            type="text"
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            placeholder="music, tech, art…"
          />
        </label>
        {error && <p style={{ color: "crimson" }}>{error}</p>}
        <button type="submit" disabled={busy || !body.trim()}>
          {busy ? "Publishing…" : "Publish"}
        </button>
      </form>
    </div>
  );
}
