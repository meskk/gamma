"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";

import type { VerificationRequest } from "@contract/VerificationRequest";

import { apiFetch } from "@/lib/api";
import { useRequireOperator } from "@/lib/useRequireOperator";

export default function VerificationPage() {
  const { token, ready, isOperator } = useRequireOperator();
  const [userId, setUserId] = useState("");
  const [verified, setVerified] = useState(true);
  const [msg, setMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setMsg(null);
    setError(null);
    const body: VerificationRequest = { verified };
    try {
      await apiFetch<unknown>(`/users/${userId}/verification`, { method: "PUT", body, token });
      setMsg(`User ${userId} set to ${verified ? "verified" : "unverified"}.`);
    } catch {
      setError("Failed — check the user id.");
    }
  }

  if (!ready || !isOperator) return null;

  return (
    <div>
      <p>
        <Link href="/admin">← Dashboard</Link>
      </p>
      <h1>User verification (bot gate)</h1>
      <p style={{ color: "#666" }}>
        The bot gate is the hard veto that decides who earns gems.
      </p>
      <form onSubmit={onSubmit} style={{ display: "grid", gap: "0.75rem", maxWidth: 320 }}>
        <label>
          User id
          <br />
          <input value={userId} onChange={(e) => setUserId(e.target.value)} required />
        </label>
        <label>
          <input
            type="checkbox"
            checked={verified}
            onChange={(e) => setVerified(e.target.checked)}
          />{" "}
          Verified (passes the bot gate)
        </label>
        <button type="submit">Apply</button>
      </form>
      {msg && <p>{msg}</p>}
      {error && <p style={{ color: "crimson" }}>{error}</p>}
    </div>
  );
}
