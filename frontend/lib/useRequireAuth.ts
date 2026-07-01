"use client";

// Redirect to /login once the session restore has finished and there's no token.
// Use on every authenticated page; it returns the auth state for convenience.

import { usePathname, useRouter } from "next/navigation";
import { useEffect } from "react";

import { useAuth } from "./auth";

export function useRequireAuth() {
  const auth = useAuth();
  const router = useRouter();
  const pathname = usePathname();

  useEffect(() => {
    if (auth.ready && !auth.token) {
      // Preserve where the user was headed so login can send them back there
      // instead of always dumping them on /feed.
      const next = pathname && pathname !== "/login" ? `?next=${encodeURIComponent(pathname)}` : "";
      router.replace(`/login${next}`);
    }
  }, [auth.ready, auth.token, router, pathname]);

  return auth;
}
