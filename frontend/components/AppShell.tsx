"use client";

// The app frame: a header with nav that adapts to auth state, around the page body.
// Functional styling only — the external designer restyles later.

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { useAuth } from "@/lib/auth";

// Auth screens bring their own full-bleed background + back button, so the app nav
// frame is skipped for them. Registration lives inside the email-first /login flow
// (its "Registrieren" tab), so there is no separate /register route.
const BARE_ROUTES = new Set(["/login"]);

export function AppShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const { token, userId, ready, logout, isOperator } = useAuth();

  if (BARE_ROUTES.has(pathname)) return <>{children}</>;

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", maxWidth: 720, margin: "0 auto", padding: "1rem" }}>
      <header
        style={{
          display: "flex",
          gap: "1rem",
          alignItems: "center",
          borderBottom: "1px solid #ddd",
          paddingBottom: "0.75rem",
          marginBottom: "1.5rem",
        }}
      >
        <Link href="/" style={{ fontWeight: 700 }}>
          Peer Network
        </Link>
        {token && (
          <>
            <Link href="/feed">Feed</Link>
            <Link href="/compose">Compose</Link>
            {userId && <Link href={`/users/${userId}`}>Profile</Link>}
            {isOperator && <Link href="/admin">Admin</Link>}
          </>
        )}
        <span style={{ marginLeft: "auto" }}>
          {!ready ? null : token ? (
            <button onClick={logout} type="button">
              Log out
            </button>
          ) : (
            <Link href="/login">Log in</Link>
          )}
        </span>
      </header>
      <main>{children}</main>
    </div>
  );
}
