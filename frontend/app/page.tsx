"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

import { useAuth } from "@/lib/auth";

// `/` is a pure entry point, not a page: signed-in users go straight to their feed,
// everyone else to the login screen. No interstitial landing (the old one flashed an
// unstyled page with header chrome before the login screen — jarring and pointless).
export default function Home() {
  const { token, ready } = useAuth();
  const router = useRouter();

  useEffect(() => {
    if (!ready) return;
    router.replace(token ? "/feed" : "/login");
  }, [ready, token, router]);

  return null;
}
