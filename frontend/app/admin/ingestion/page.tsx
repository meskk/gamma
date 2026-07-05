"use client";

import Link from "next/link";
import { useState } from "react";

import type { BackfillResult } from "@contract/BackfillResult";
import type { IngestionStatus } from "@contract/IngestionStatus";

import { apiFetch } from "@/lib/api";
import { useFetch } from "@/lib/useFetch";
import { useRequireOperator } from "@/lib/useRequireOperator";

export default function IngestionPage() {
  const { token, ready, isOperator } = useRequireOperator();
  // Load errors stay silent (as before): the page is a status display; the
  // backfill button has its own message.
  const { data: status, reload } = useFetch<IngestionStatus>(
    () => apiFetch("/admin/ingestion/status", { token }),
    [token],
    { enabled: !!token },
  );
  const [msg, setMsg] = useState<string | null>(null);

  async function backfill() {
    setMsg(null);
    try {
      const r = await apiFetch<BackfillResult>("/admin/ingestion/backfill", {
        method: "POST",
        token,
      });
      setMsg(`Enqueued ${String(r.enqueued)} posts (cursor ${String(r.last_id)}).`);
      reload();
    } catch {
      setMsg("Backfill failed.");
    }
  }

  if (!ready || !isOperator) return null;

  const byVersion = status
    ? Object.entries(status.by_model_version)
        .map(([k, v]) => `${k}: ${String(v)}`)
        .join(", ")
    : "";

  return (
    <div>
      <p>
        <Link href="/admin">← Dashboard</Link>
      </p>
      <h1>Ingestion status</h1>
      {status && (
        <ul>
          <li>Analyzed: {String(status.analyzed)}</li>
          <li>Unanalyzed: {String(status.unanalyzed)}</li>
          <li>By model version: {byVersion || "—"}</li>
        </ul>
      )}
      <button type="button" onClick={backfill}>
        Backfill a page of unanalyzed posts
      </button>
      {msg && <p>{msg}</p>}
    </div>
  );
}
