"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";

import type { ReportedPost } from "@contract/ReportedPost";

import { apiFetch } from "@/lib/api";
import { useRequireOperator } from "@/lib/useRequireOperator";

export default function ReportsPage() {
  const { token, ready, isOperator } = useRequireOperator();
  const [reports, setReports] = useState<ReportedPost[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!token) return;
    apiFetch<ReportedPost[]>("/reports", { token })
      .then(setReports)
      .catch(() => setError("Could not load the moderation queue."));
  }, [token]);

  useEffect(() => {
    load();
  }, [load]);

  async function act(postId: string, action: "takedown" | "restore") {
    try {
      await apiFetch<unknown>(`/posts/${postId}/${action}`, { method: "POST", token });
      load();
    } catch {
      setError("Action failed.");
    }
  }

  if (!ready || !isOperator) return null;

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
