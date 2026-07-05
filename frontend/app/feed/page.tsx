"use client";

// The feed is the app's landing screen after login: a full-screen reels view
// (Figma "Glass · Reels"). It is a BARE route (no app-shell chrome — see
// AppShell.tsx) because it brings its own full-bleed background and bottom nav.

import { ReelsFeed } from "@/components/reels/ReelsFeed";
import { useRequireAuth } from "@/lib/useRequireAuth";

export default function FeedPage() {
  const { token, userId, ready } = useRequireAuth();

  if (!ready || !token || !userId) return null;

  return <ReelsFeed token={token} userId={userId} />;
}
