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
import type { RegisterRequest } from "@contract/RegisterRequest";
import type { Role } from "@contract/Role";

import { apiFetch } from "./api";

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
      .catch(() => sessionStorage.removeItem(TOKEN_KEY))
      .finally(() => setReady(true));
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

  function logout() {
    sessionStorage.removeItem(TOKEN_KEY);
    setToken(null);
    setUserId(null);
    setRole(null);
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
