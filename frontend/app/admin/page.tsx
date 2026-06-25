"use client";

import Link from "next/link";

import { useRequireOperator } from "@/lib/useRequireOperator";

export default function AdminHome() {
  const { ready, isOperator } = useRequireOperator();
  if (!ready || !isOperator) return null;

  return (
    <div>
      <h1>Operator dashboard</h1>
      <ul style={{ lineHeight: 1.9 }}>
        <li>
          <Link href="/admin/reports">Moderation queue</Link> — review reported posts
        </li>
        <li>
          <Link href="/admin/verification">User verification</Link> — set the bot gate
        </li>
        <li>
          <Link href="/admin/settle">Epoch settlement</Link> — mint &amp; distribute gems
        </li>
        <li>
          <Link href="/admin/ingestion">Ingestion status</Link> — analysis progress &amp; backfill
        </li>
      </ul>
    </div>
  );
}
