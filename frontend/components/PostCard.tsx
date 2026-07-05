import Link from "next/link";

import type { Post } from "@contract/Post";

// A compact post summary used in the feed. (ids are bigint in the contract but
// numbers at runtime — String() renders both.)
export function PostCard({ post }: { post: Post }) {
  return (
    <article
      style={{
        border: "1px solid #eee",
        borderRadius: 8,
        padding: "0.75rem 1rem",
        marginBottom: "0.75rem",
      }}
    >
      <div style={{ fontSize: "0.8rem", color: "#888" }}>
        <Link href={`/users/${post.author_id}`}>@user-{String(post.author_id)}</Link>
        {post.category ? ` · ${post.category}` : ""}
        {" · "}
        {new Date(post.created_at).toLocaleString()}
      </div>
      <p style={{ margin: "0.4rem 0", whiteSpace: "pre-wrap" }}>
        {post.body ?? <em>(kein Text)</em>}
      </p>
      {post.media_id != null && (
        <p style={{ margin: "0.2rem 0", fontSize: "0.8rem", color: "#888" }}>📎 mit Medien</p>
      )}
      <Link href={`/posts/${post.id}`} style={{ fontSize: "0.85rem" }}>
        Öffnen →
      </Link>
    </article>
  );
}
