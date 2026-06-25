"use client";

// Redirect to /login once the session restore has finished and there's no token.
// Use on every authenticated page; it returns the auth state for convenience.

import { useRouter } from "next/navigation";
import { useEffect } from "react";

import { useAuth } from "./auth";

export function useRequireAuth() {
  const auth = useAuth();
  const router = useRouter();

  useEffect(() => {
    if (auth.ready && !auth.token) {
      router.replace("/login");
    }
  }, [auth.ready, auth.token, router]);

  return auth;
}
