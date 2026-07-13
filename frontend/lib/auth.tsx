"use client";

// Auth session: holds the bearer token (in sessionStorage) and the current user's id
// + role, restores the session on load via GET /auth/me, and exposes login/register/
// logout. Token storage is sessionStorage + Bearer (Phase-1a in-house choice); a 401
// anywhere is treated as "logged out". The role gates operator-only UI.

import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

import type { AuthResponse } from "@contract/AuthResponse";
import type { CurrentUser } from "@contract/CurrentUser";
import type { LoginRequest } from "@contract/LoginRequest";
import type { LoginWithCodeRequest } from "@contract/LoginWithCodeRequest";
import type { RegisterRequest } from "@contract/RegisterRequest";
import type { ResetPasswordRequest } from "@contract/ResetPasswordRequest";
import type { Role } from "@contract/Role";

import { ApiError, apiFetch } from "./api";

type AuthState = {
  token: string | null;
  /** The current user's id as a string (the contract's bigint, stringified). */
  userId: string | null;
  role: Role | null;
  isOperator: boolean;
  /** True once the initial session restore has finished. */
  ready: boolean;
  login: (email: string, password: string) => Promise<void>;
  register: (req: RegisterRequest) => Promise<void>;
  /** Passwordless login: exchange an emailed code for a session. */
  loginWithCode: (email: string, code: string) => Promise<void>;
  /** Set a new password with an emailed reset code; returns a fresh session. */
  resetPassword: (email: string, code: string, newPassword: string) => Promise<void>;
  logout: () => void;
};

const AuthContext = createContext<AuthState | null>(null);
const TOKEN_KEY = "gamma_token";

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(null);
  const [userId, setUserId] = useState<string | null>(null);
  const [role, setRole] = useState<Role | null>(null);
  const [ready, setReady] = useState(false);

  async function loadSession(t: string) {
    const me = await apiFetch<CurrentUser>("/auth/me", { token: t });
    // Deliberate Phase-1a decision: the bearer token lives in sessionStorage
    // (JS-readable) rather than an HttpOnly cookie, to keep the SPA + Bearer flow
    // simple. The compensating control is the Content-Security-Policy set in
    // next.config.mjs, which constrains script/connect origins to shrink the XSS
    // token-exfiltration surface. Revisit (HttpOnly cookie) if/when this hardens.
    sessionStorage.setItem(TOKEN_KEY, t);
    setToken(t);
    setUserId(String(me.user_id));
    setRole(me.role);
  }

  useEffect(() => {
    const stored = sessionStorage.getItem(TOKEN_KEY);
    if (!stored) {
      setReady(true);
      return;
    }
    loadSession(stored)
      // Only evict the stored token on a real 401 (the session is invalid). A
      // transient 5xx/network blip must NOT log a valid user out.
      .catch((e) => {
        if (e instanceof ApiError && e.status === 401) {
          sessionStorage.removeItem(TOKEN_KEY);
        }
      })
      .finally(() => setReady(true));
  }, []);

  // A 401 on any AUTHENTICATED request (signalled by api.ts) means the session is
  // already dead server-side — just clear local state. Uses clearSession (NOT
  // logout), so it never fires another request and can't loop.
  useEffect(() => {
    if (typeof window === "undefined") return;
    const onUnauthorized = () => clearSession();
    window.addEventListener("gamma:unauthorized", onUnauthorized);
    return () => window.removeEventListener("gamma:unauthorized", onUnauthorized);
  }, []);

  async function login(email: string, password: string) {
    const body: LoginRequest = { email, password };
    const resp = await apiFetch<AuthResponse>("/auth/login", { method: "POST", body });
    await loadSession(resp.token);
  }

  async function register(req: RegisterRequest) {
    const resp = await apiFetch<AuthResponse>("/auth/register", { method: "POST", body: req });
    await loadSession(resp.token);
  }

  async function loginWithCode(email: string, code: string) {
    const body: LoginWithCodeRequest = { email, code };
    const resp = await apiFetch<AuthResponse>("/auth/login-with-code", { method: "POST", body });
    await loadSession(resp.token);
  }

  async function resetPassword(email: string, code: string, newPassword: string) {
    const body: ResetPasswordRequest = { email, code, new_password: newPassword };
    const resp = await apiFetch<AuthResponse>("/auth/reset-password", { method: "POST", body });
    await loadSession(resp.token);
  }

  // Clear local session state only (no request). Used by the 401 handler, where
  // the server session is already gone.
  function clearSession() {
    sessionStorage.removeItem(TOKEN_KEY);
    setToken(null);
    setUserId(null);
    setRole(null);
  }

  // User-initiated logout: REVOKE the session server-side (POST /auth/logout) so a
  // leaked/copied token can't be replayed for the rest of its 30-day life, then
  // clear locally. Best-effort — an already-invalid token or a network blip still
  // logs the user out locally.
  async function logout() {
    const t = sessionStorage.getItem(TOKEN_KEY);
    if (t) {
      try {
        await apiFetch("/auth/logout", { method: "POST", token: t });
      } catch {
        // token already invalid or backend unreachable — the local clear is enough
      }
    }
    clearSession();
  }

  return (
    <AuthContext.Provider
      value={{
        token,
        userId,
        role,
        isOperator: role === "operator",
        ready,
        login,
        register,
        loginWithCode,
        resetPassword,
        logout,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within an AuthProvider");
  return ctx;
}
