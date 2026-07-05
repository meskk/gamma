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
