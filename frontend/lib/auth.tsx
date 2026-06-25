"use client";

// Auth session: holds the bearer token (in sessionStorage) and the current user id,
// restores the session on load via GET /auth/me, and exposes login/register/logout.
// Token storage is sessionStorage + Bearer header (Phase-1a in-house choice); a 401
// anywhere is treated as "logged out". (Role/operator awareness is added in M7.)

import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

import type { AuthResponse } from "@contract/AuthResponse";
import type { LoginRequest } from "@contract/LoginRequest";
import type { RegisterRequest } from "@contract/RegisterRequest";

import { apiFetch } from "./api";

type Me = { user_id: bigint };

type AuthState = {
  token: string | null;
  /** The current user's id as a string (the contract's bigint, stringified). */
  userId: string | null;
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
  const [ready, setReady] = useState(false);

  useEffect(() => {
    const stored = sessionStorage.getItem(TOKEN_KEY);
    if (!stored) {
      setReady(true);
      return;
    }
    apiFetch<Me>("/auth/me", { token: stored })
      .then((me) => {
        setToken(stored);
        setUserId(String(me.user_id));
      })
      .catch(() => sessionStorage.removeItem(TOKEN_KEY))
      .finally(() => setReady(true));
  }, []);

  function adopt(resp: AuthResponse) {
    sessionStorage.setItem(TOKEN_KEY, resp.token);
    setToken(resp.token);
    setUserId(String(resp.user_id));
  }

  async function login(email: string, password: string) {
    const body: LoginRequest = { email, password };
    adopt(await apiFetch<AuthResponse>("/auth/login", { method: "POST", body }));
  }

  async function register(req: RegisterRequest) {
    adopt(await apiFetch<AuthResponse>("/auth/register", { method: "POST", body: req }));
  }

  function logout() {
    sessionStorage.removeItem(TOKEN_KEY);
    setToken(null);
    setUserId(null);
  }

  return (
    <AuthContext.Provider value={{ token, userId, ready, login, register, logout }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within an AuthProvider");
  return ctx;
}
