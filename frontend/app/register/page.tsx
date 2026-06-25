"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { ApiError } from "@/lib/api";
import { useAuth } from "@/lib/auth";

export default function RegisterPage() {
  const { register } = useAuth();
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [categories, setCategories] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const declared_categories = categories
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      await register({ email, password, declared_categories });
      router.push("/feed");
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 409
          ? "That email is already registered."
          : "Registration failed — please try again.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h1>Create an account</h1>
      <form onSubmit={onSubmit} style={{ display: "grid", gap: "0.75rem", maxWidth: 320 }}>
        <label>
          Email
          <br />
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
        </label>
        <label>
          Password
          <br />
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            minLength={8}
          />
        </label>
        <label>
          Interests <small>(comma-separated, optional)</small>
          <br />
          <input
            type="text"
            value={categories}
            onChange={(e) => setCategories(e.target.value)}
            placeholder="music, tech, art"
          />
        </label>
        {error && <p style={{ color: "crimson" }}>{error}</p>}
        <button type="submit" disabled={busy}>
          {busy ? "…" : "Create account"}
        </button>
      </form>
      <p>
        Already have an account? <Link href="/login">Log in</Link>.
      </p>
    </div>
  );
}
