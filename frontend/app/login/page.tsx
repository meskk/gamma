"use client";

// Glass auth (design: Figma "Social App – Design System", 🫧 Glass · Login). The
// visual is the designer's; the data flow uses the existing auth.
//   - Anmelden / Registrieren tabs pick the mode (matching the design).
//   - Step 1: enter email → "Weiter". We call /auth/check-email so a wrong tab is
//     caught nicely (e.g. "Anmelden" with an unknown email → hint to register).
//   - Step 2: same glass card, password field → sign in / create the account.
// No passkey / no email-code / no wallet (not in the backend); the email-only
// "Methoden-Wähler" from the design collapses to a direct email field.

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import type { EmailCheckResult } from "@contract/EmailCheckResult";

import { ApiError, apiFetch } from "@/lib/api";
import { useAuth } from "@/lib/auth";

type Step = "email" | "password";
type Mode = "login" | "register";

function MailIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="3" y="5" width="18" height="14" rx="3" />
      <path d="m3.5 7 8.5 6 8.5-6" />
    </svg>
  );
}

function LockIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="5" y="11" width="14" height="9" rx="2.5" />
      <path d="M8 11V8a4 4 0 0 1 8 0v3" />
    </svg>
  );
}

function SparkleIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M12 2c.45 4.95 1.55 6.05 6.5 6.5-4.95.45-6.05 1.55-6.5 6.5-.45-4.95-1.55-6.05-6.5-6.5 4.95-.45 6.05-1.55 6.5-6.5Z" />
    </svg>
  );
}

function BackIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M15 6l-6 6 6 6" />
    </svg>
  );
}

export default function LoginPage() {
  const { login, register } = useAuth();
  const router = useRouter();
  const [mode, setMode] = useState<Mode>("login");
  const [step, setStep] = useState<Step>("email");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function switchMode(next: Mode) {
    if (next === mode) return;
    setMode(next);
    setStep("email");
    setPassword("");
    setError(null);
  }

  function backToEmail() {
    setStep("email");
    setPassword("");
    setError(null);
  }

  // Step 1: validate the email against the chosen tab, then move to the password.
  async function onEmailSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const { exists } = await apiFetch<EmailCheckResult>("/auth/check-email", {
        method: "POST",
        body: { email },
      });
      if (mode === "login" && !exists) {
        setError("Mit dieser E-Mail gibt es noch kein Konto — wechsle zu „Registrieren“.");
        return;
      }
      if (mode === "register" && exists) {
        setError("Diese E-Mail ist bereits registriert — wechsle zu „Anmelden“.");
        return;
      }
      setStep("password");
    } catch {
      setError("Etwas ist schiefgelaufen — bitte erneut versuchen.");
    } finally {
      setBusy(false);
    }
  }

  // Step 2: sign in, or create the account.
  async function onPasswordSubmit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (mode === "login") {
        await login(email, password);
      } else {
        await register({ email, password, declared_categories: [] });
      }
      router.push("/feed");
    } catch (err) {
      if (mode === "login") {
        setError(
          err instanceof ApiError && err.status === 401
            ? "Falsches Passwort."
            : "Anmeldung fehlgeschlagen — bitte erneut versuchen.",
        );
      } else {
        setError(
          err instanceof ApiError && err.status === 400
            ? "Passwort zu kurz (mindestens 8 Zeichen)."
            : "Registrierung fehlgeschlagen — bitte erneut versuchen.",
        );
      }
    } finally {
      setBusy(false);
    }
  }

  const seg = (active: boolean) => ({
    flex: 1,
    padding: "9px 0",
    borderRadius: 999,
    border: "none",
    cursor: "pointer",
    fontSize: 14,
    fontWeight: active ? 600 : 500,
    color: active ? "#fff" : "rgba(255,255,255,0.55)",
    background: active ? "rgba(255,255,255,0.18)" : "transparent",
    transition: "background 0.15s, color 0.15s",
  });
  const field = {
    display: "flex",
    alignItems: "center",
    gap: 10,
    width: "100%",
    boxSizing: "border-box" as const,
    padding: "14px 16px",
    borderRadius: 14,
    background: "rgba(255,255,255,0.08)",
    border: "1px solid rgba(255,255,255,0.18)",
    transition: "border-color 0.15s, background 0.15s",
  };
  const input = { flex: 1, minWidth: 0, background: "transparent", border: "none", outline: "none", color: "#fff", fontSize: 15 };
  const heading = { margin: 0, fontFamily: "'Hanken Grotesk', 'Inter', sans-serif", fontWeight: 700, fontSize: 26, color: "#fff", textAlign: "center" as const };
  const subtext = { margin: 0, fontSize: 14, color: "rgba(255,255,255,0.55)", textAlign: "center" as const };

  return (
    <div className="glass-login">
      <style>{`
        @import url('https://fonts.googleapis.com/css2?family=Hanken+Grotesk:wght@600;700&family=Inter:wght@400;500;600&display=swap');
        html, body { margin: 0; }
        .glass-login { font-family: 'Inter', system-ui, -apple-system, sans-serif; }
        .glass-login input::placeholder { color: rgba(255,255,255,0.4); }
        .glass-login .gl-field:focus-within { border-color: rgba(255,255,255,0.42); background: rgba(255,255,255,0.11); }
        .glass-login .gl-primary:hover:not(:disabled) { background: rgba(255,255,255,0.30); }
        .glass-login .gl-primary:disabled { opacity: 0.6; cursor: default; }
        .glass-login .gl-back:hover { background: rgba(255,255,255,0.18); }
        .glass-login .gl-change { background: none; border: none; padding: 0; cursor: pointer; color: rgba(255,255,255,0.85); text-decoration: underline; font: inherit; }
      `}</style>

      <div
        style={{
          position: "relative",
          minHeight: "100vh",
          width: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: 16,
          background: "linear-gradient(180deg, #0d0d10 0%, #060608 100%)",
          overflow: "hidden",
        }}
      >
        <div aria-hidden style={{ position: "absolute", width: 820, height: 820, left: "50%", top: 60, transform: "translateX(-50%)", background: "radial-gradient(circle, rgba(124,92,255,0.20) 0%, rgba(124,92,255,0) 70%)", filter: "blur(20px)", pointerEvents: "none" }} />
        <div aria-hidden style={{ position: "absolute", width: 520, height: 520, left: "55%", top: 520, transform: "translateX(-50%)", background: "radial-gradient(circle, rgba(64,120,255,0.18) 0%, rgba(64,120,255,0) 70%)", filter: "blur(20px)", pointerEvents: "none" }} />

        <button
          type="button"
          className="gl-back"
          onClick={() => (step === "password" ? backToEmail() : router.back())}
          aria-label="Zurück"
          style={{ position: "absolute", top: 24, left: 24, width: 36, height: 36, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 999, background: "rgba(255,255,255,0.10)", border: "1px solid rgba(255,255,255,0.22)", color: "rgba(255,255,255,0.85)", cursor: "pointer", transition: "background 0.15s" }}
        >
          <BackIcon />
        </button>

        <form
          onSubmit={step === "email" ? onEmailSubmit : onPasswordSubmit}
          style={{
            position: "relative",
            width: "min(440px, 100%)",
            boxSizing: "border-box",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 16,
            padding: "44px 40px 40px",
            borderRadius: 30,
            background: "rgba(255,255,255,0.06)",
            border: "1.2px solid rgba(255,255,255,0.16)",
            boxShadow: "0 24px 60px rgba(0,0,0,0.55)",
            backdropFilter: "blur(24px)",
            WebkitBackdropFilter: "blur(24px)",
          }}
        >
          {/* Logo */}
          <div style={{ display: "flex", alignItems: "center", gap: 10, paddingBottom: 6 }}>
            <span style={{ width: 34, height: 34, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 10, background: "rgba(255,255,255,0.16)", border: "1px solid rgba(255,255,255,0.35)", color: "#fff" }}>
              <SparkleIcon />
            </span>
            <span style={{ fontFamily: "'Hanken Grotesk', 'Inter', sans-serif", fontWeight: 700, fontSize: 22, color: "#fff" }}>
              Poolside
            </span>
          </div>

          {/* Anmelden / Registrieren tabs */}
          <div style={{ display: "flex", gap: 4, width: "100%", padding: 4, borderRadius: 999, background: "rgba(255,255,255,0.07)", boxSizing: "border-box" }}>
            <button type="button" style={seg(mode === "login")} onClick={() => switchMode("login")}>
              Anmelden
            </button>
            <button type="button" style={seg(mode === "register")} onClick={() => switchMode("register")}>
              Registrieren
            </button>
          </div>

          <h1 style={heading}>{mode === "login" ? "Willkommen zurück" : "Konto erstellen"}</h1>
          <p style={subtext}>
            {step === "email" ? (
              mode === "login" ? "Gib deine E-Mail ein, um dich anzumelden." : "Gib deine E-Mail ein, um zu starten."
            ) : (
              <>
                {email}
                {" · "}
                <button type="button" className="gl-change" onClick={backToEmail}>
                  Ändern
                </button>
              </>
            )}
          </p>

          {step === "email" ? (
            <div className="gl-field" style={field}>
              <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }}>
                <MailIcon />
              </span>
              <input
                type="email"
                required
                autoFocus
                autoComplete="email"
                placeholder="du@email.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                style={input}
              />
            </div>
          ) : (
            <div className="gl-field" style={field}>
              <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }}>
                <LockIcon />
              </span>
              <input
                type="password"
                required
                autoFocus
                minLength={mode === "register" ? 8 : undefined}
                autoComplete={mode === "login" ? "current-password" : "new-password"}
                placeholder={mode === "login" ? "Passwort" : "Passwort wählen (min. 8 Zeichen)"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                style={input}
              />
            </div>
          )}

          {error && <p style={{ margin: 0, width: "100%", fontSize: 13, color: "#ff8585", textAlign: "center" }}>{error}</p>}

          <button
            type="submit"
            className="gl-primary"
            disabled={busy}
            style={{ width: "100%", padding: "15px 0", borderRadius: 999, background: "rgba(255,255,255,0.22)", border: "1.2px solid rgba(255,255,255,0.42)", boxShadow: "0 8px 20px rgba(0,0,0,0.4)", color: "#fff", fontSize: 15, fontWeight: 500, cursor: "pointer", transition: "background 0.15s" }}
          >
            {busy ? "…" : step === "email" ? "Weiter" : mode === "login" ? "Anmelden" : "Konto erstellen"}
          </button>

          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, paddingTop: 6, fontSize: 11 }}>
            <span style={{ color: "rgba(255,255,255,0.38)", textAlign: "center" }}>Mit Fortfahren akzeptierst du unsere</span>
            <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <span style={{ color: "rgba(255,255,255,0.62)", fontWeight: 500 }}>AGB</span>
              <span style={{ color: "rgba(255,255,255,0.3)" }}>·</span>
              <span style={{ color: "rgba(255,255,255,0.62)", fontWeight: 500 }}>Datenschutz</span>
            </span>
          </div>
        </form>
      </div>
    </div>
  );
}
