"use client";

// Guard for /admin pages: send unauthenticated users to /login and non-operators to
// /feed, once the session restore has finished. Returns the auth state.

import { useRouter } from "next/navigation";
import { useEffect } from "react";

import { useAuth } from "./auth";

export function useRequireOperator() {
  const auth = useAuth();
  const router = useRouter();

  useEffect(() => {
    if (!auth.ready) return;
    if (!auth.token) router.replace("/login");
    else if (!auth.isOperator) router.replace("/feed");
  }, [auth.ready, auth.token, auth.isOperator, router]);

  return auth;
}
