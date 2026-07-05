"use client";

import Link from "next/link";
import { useState } from "react";

import type { ReportedPost } from "@contract/ReportedPost";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { useRequireOperator } from "@/lib/useRequireOperator";

export default function ReportsPage() {
  const { token, ready, isOperator } = useRequireOperator();
  // useFetch adds the stale-guard this page never had (a latent bug class),
  // and load-after-action becomes reload().
  const {
    data: reports,
    error: loadError,
    reload,
  } = useFetch<ReportedPost[]>(() => apiFetch("/reports", { token }), [token], {
    enabled: !!token,
  });
  const [actionError, setActionError] = useState<string | null>(null);

  async function act(postId: string, action: "takedown" | "restore") {
    try {
      await apiFetch<unknown>(`/posts/${postId}/${action}`, { method: "POST", token });
      reload();
    } catch {
      setActionError("Action failed.");
    }
  }

  if (!ready || !isOperator) return null;

  const error = loadError ? "Could not load the moderation queue." : actionError;

  return (
    <div>
      <p>
        <Link href="/admin">← Dashboard</Link>
      </p>
      <h1>Moderation queue</h1>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {reports === null && <p>Loading…</p>}
      {reports !== null && reports.length === 0 && <p>No reported posts. 🎉</p>}
      {reports?.map((r) => (
        <div
          key={String(r.post_id)}
          style={{
            display: "flex",
            gap: "0.75rem",
            alignItems: "center",
            borderBottom: "1px solid #eee",
            padding: "0.5rem 0",
          }}
        >
          <Link href={`/posts/${r.post_id}`}>post {String(r.post_id)}</Link>
          <span style={{ color: "#888" }}>
            {String(r.report_count)} reports{r.hidden ? " · hidden" : ""}
          </span>
          <span style={{ marginLeft: "auto" }}>
            {r.hidden ? (
              <button type="button" onClick={() => act(String(r.post_id), "restore")}>
                Restore
              </button>
            ) : (
              <button type="button" onClick={() => act(String(r.post_id), "takedown")}>
                Take down
              </button>
            )}
          </span>
        </div>
      ))}
    </div>
  );
}
