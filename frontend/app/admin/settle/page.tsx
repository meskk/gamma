"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";

import type { SettlementSummary } from "@contract/SettlementSummary";

import { apiFetch } from "@/lib/api";
import { useRequireOperator } from "@/lib/useRequireOperator";

export default function SettlePage() {
  const { token, ready, isOperator } = useRequireOperator();
  const [epoch, setEpoch] = useState("");
  const [summary, setSummary] = useState<SettlementSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setSummary(null);
    try {
      const s = await apiFetch<SettlementSummary>(`/epochs/${epoch}/settle`, {
        method: "POST",
        token,
      });
      setSummary(s);
    } catch {
      setError("Settlement failed (already settled, or a future epoch?).");
    }
  }

  if (!ready || !isOperator) return null;

  return (
    <div>
      <p>
        <Link href="/admin">← Dashboard</Link>
      </p>
      <h1>Epoch settlement</h1>
      <form onSubmit={onSubmit} style={{ display: "flex", gap: "0.5rem" }}>
        <input
          value={epoch}
          onChange={(e) => setEpoch(e.target.value)}
          placeholder="epoch k"
          required
        />
        <button type="submit">Settle</button>
      </form>
      {error && <p style={{ color: "crimson" }}>{error}</p>}
      {summary && (
        <ul>
          <li>Epoch {String(summary.epoch_k)}</li>
          <li>Emission: {String(summary.emission)} PT</li>
          <li>Participants: {summary.user_count}</li>
          <li>{summary.already_settled ? "Already settled (no-op)." : "Settled now."}</li>
        </ul>
      )}
    </div>
  );
}
