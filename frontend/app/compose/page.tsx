"use client";

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import type { NewPost } from "@contract/NewPost";
import type { Post } from "@contract/Post";

import { apiFetch } from "@/lib/api";
import { FEATURES } from "@/lib/features";
import type { Wire } from "@/lib/wire";
import { uploadMedia } from "@/lib/mediaUpload";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function ComposePage() {
  const { token, ready } = useRequireAuth();
  const router = useRouter();
  const [body, setBody] = useState("");
  const [category, setCategory] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [price, setPrice] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!token) return;
    setBusy(true);
    setError(null);
    try {
      // Upload the attachment first (presigned flow), then reference it on the post.
      let mediaId: number | null = null;
      if (file) {
        // With gem unlocks hidden (P-1 launch matrix), every upload is free.
        const asset = await uploadMedia(file, FEATURES.gemUnlock ? price : 0, token);
        mediaId = Number(asset.id);
      }
      const payload: Wire<NewPost> = {
        body: body.trim(),
        category: category.trim() ? category.trim() : null,
        media_id: mediaId, // bigint|null in the contract; a number on the wire
      };
      const created = await apiFetch<Post>("/posts", { method: "POST", body: payload, token });
      router.push(`/posts/${created.id}`);
    } catch {
      setError(
        file ? "Could not upload the media or publish — please try again." : "Could not publish.",
      );
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
        <label>
          Attach media <small>(optional — image / video / audio)</small>
          <br />
          <input
            type="file"
            accept="image/*,video/*,audio/*"
            onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          />
        </label>
        {file && FEATURES.gemUnlock && (
          <label>
            Unlock price <small>(gems; 0 = free)</small>
            <br />
            <input
              type="number"
              min={0}
              value={price}
              onChange={(e) => setPrice(Math.max(0, Number(e.target.value) || 0))}
            />
          </label>
        )}
        {error && <p style={{ color: "crimson" }}>{error}</p>}
        <button type="submit" disabled={busy || !body.trim()}>
          {busy ? "Publishing…" : "Publish"}
        </button>
      </form>
    </div>
  );
}
