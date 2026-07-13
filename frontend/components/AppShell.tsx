"use client";

// The app frame: a header with nav that adapts to auth state, around the page body.
// Functional styling only — the external designer restyles later.

import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

import { useAuth } from "@/lib/auth";

// Full-bleed screens that bring their own background + navigation, so the app nav
// frame is skipped for them: the auth screens (/login — registration is its
// "Registrieren" tab), the redirecting entry point (/), and the reels feed (/feed,
// which has its own bottom nav — the Figma "Glass · Reels" design).
const BARE_ROUTES = new Set(["/", "/login", "/feed"]);

// User profiles (/users/:id) are the Figma "Glass · Profile" screen: full-bleed
// with their own glass bottom nav, so they skip the app-shell chrome too.
function isBareRoute(pathname: string): boolean {
  return BARE_ROUTES.has(pathname) || pathname.startsWith("/users/");
}

export function AppShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const { token, userId, ready, logout, isOperator } = useAuth();

  if (isBareRoute(pathname)) return <>{children}</>;

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
          Poolsite
        </Link>
        {token && (
          <>
            <Link href="/feed">Feed</Link>
            <Link href="/compose">Erstellen</Link>
            {userId && <Link href={`/users/${userId}`}>Profil</Link>}
            {isOperator && <Link href="/admin">Admin</Link>}
          </>
        )}
        <span style={{ marginLeft: "auto" }}>
          {!ready ? null : token ? (
            <button onClick={logout} type="button">
              Abmelden
            </button>
          ) : (
            <Link href="/login">Anmelden</Link>
          )}
        </span>
      </header>
      <main>{children}</main>
    </div>
  );
}
