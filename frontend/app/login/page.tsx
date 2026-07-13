"use client";

// Glass auth (design: Figma "Social App – Design System", 🫧 Glass · Login). The
// visual is the designer's; the data flow uses the existing auth.
//   - Anmelden / Registrieren tabs pick the mode (matching the design).
//   - Step 1 (email): enter email → "Weiter". We call /auth/check-email so a wrong
//     tab is caught nicely (login with an unknown email → hint to register). The
//     check is a UX nicety, not an auth precondition: if it fails we degrade to the
//     password step rather than dead-ending.
//   - Step 2 (password): same glass card, password field → sign in, or (register)
//     advance to the interests step.
//   - Step 3 (interests, register only): capture declared_categories for cold-start,
//     then create the account. This is the ONE registration flow (the old English
//     /register plain-form was removed — see AppShell / app/page.tsx).
// Recovery (backend: email one-time codes): from the login password step you can
// either sign in with an emailed code (passwordless) or reset a forgotten
// password — both go through a shared "code" step.

import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect, useRef, useState, type FormEvent } from "react";

import type { EmailCheckRequest } from "@contract/EmailCheckRequest";
import type { EmailCheckResult } from "@contract/EmailCheckResult";
import type { RequestCodeRequest } from "@contract/RequestCodeRequest";

import { ApiError, apiFetch } from "@/lib/api";
import { useAuth } from "@/lib/auth";

type Step = "email" | "password" | "interests" | "code";
type Mode = "login" | "register";
// Recovery sub-mode once on the "code" step: exchange the code for a session, or
// use it to set a new password.
type Recovery = "login" | "reset";

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

function TagIcon() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M20.6 13.4 12 22l-8-8 8.6-8.6a2 2 0 0 1 1.4-.6H20a2 2 0 0 1 2 2v6a2 2 0 0 1-.6 1.4Z" />
      <circle cx="16.5" cy="7.5" r="1.2" fill="currentColor" stroke="none" />
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
  // useSearchParams() (for ?next=) requires a Suspense boundary under Next's
  // static generation.
  return (
    <Suspense fallback={null}>
      <LoginForm />
    </Suspense>
  );
}

function LoginForm() {
  const { login, register, loginWithCode, resetPassword } = useAuth();
  const router = useRouter();
  const searchParams = useSearchParams();
  const [mode, setMode] = useState<Mode>("login");
  const [step, setStep] = useState<Step>("email");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [interests, setInterests] = useState("");
  // Recovery state (the "code" step). `recovery` picks login-vs-reset; `code`
  // and `newPassword` are the fields on that step.
  const [recovery, setRecovery] = useState<Recovery>("login");
  const [code, setCode] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [notice, setNotice] = useState<string | null>(null);
  // Referral code from a shared invite link (/login?ref=CODE, P-2). Held in
  // state so an INVALID code can be dropped after its error — the person can
  // still register, they just don't credit anyone.
  const [refCode, setRefCode] = useState<string | null>(() => searchParams.get("ref"));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // In-flight request generation. Every state transition (tab switch, back) bumps
  // it; an async handler captures the value at submit time and drops its result if
  // the flow has moved on — this defeats the stale-closure race where a slow
  // check-email/login response lands after the user changed tabs or stepped back.
  const flowGen = useRef(0);
  // Rate-limit cooldown (server 429 + Retry-After). Remaining whole seconds in
  // state (drives the button countdown); the actual deadline in a ref so ticks
  // recompute from the clock instead of drifting. DELIBERATELY not reset by tab
  // switches or flowGen bumps — the server-side limit is real either way.
  const [cooldownLeft, setCooldownLeft] = useState(0);
  const cooldownDeadline = useRef(0);
  const cooldownActive = cooldownLeft > 0;

  function startCooldown(secs: number) {
    cooldownDeadline.current = Date.now() + secs * 1000;
    setCooldownLeft(Math.max(1, secs));
  }

  useEffect(() => {
    if (!cooldownActive) return;
    const id = window.setInterval(() => {
      setCooldownLeft(Math.max(0, Math.ceil((cooldownDeadline.current - Date.now()) / 1000)));
    }, 250);
    return () => window.clearInterval(id);
  }, [cooldownActive]);

  /** Shared 429 handling for the real submit steps: start the countdown and show
   * a STATIC alert (the ticking number lives only in the button label, so screen
   * readers aren't spammed every second). */
  function handleRateLimit(err: unknown): boolean {
    if (err instanceof ApiError && err.status === 429) {
      startCooldown(err.retryAfter ?? 30);
      setError("Zu viele Versuche — bitte warte kurz.");
      return true;
    }
    return false;
  }

  // Redirect target after a successful login (finding: preserve intended URL). Only
  // honour local paths — never an absolute/protocol-relative URL (open-redirect).
  function redirectTarget(): string {
    const next = searchParams.get("next");
    if (next && next.startsWith("/") && !next.startsWith("//")) return next;
    return "/feed";
  }

  function resetRecoveryFields() {
    setCode("");
    setNewPassword("");
    setNotice(null);
  }

  function switchMode(next: Mode) {
    if (next === mode || busy) return;
    flowGen.current += 1; // invalidate any in-flight request
    setMode(next);
    setStep("email");
    setPassword("");
    setInterests("");
    resetRecoveryFields();
    setError(null);
  }

  function backToEmail() {
    if (busy) return;
    flowGen.current += 1;
    setStep("email");
    setPassword("");
    resetRecoveryFields();
    setError(null);
  }

  function backToPassword() {
    if (busy) return;
    flowGen.current += 1;
    setStep("password");
    resetRecoveryFields();
    setError(null);
  }

  // Step 1: validate the email against the chosen tab, then move to the password.
  async function onEmailSubmit(e: FormEvent) {
    e.preventDefault();
    const gen = ++flowGen.current;
    setBusy(true);
    setError(null);
    try {
      const body: EmailCheckRequest = { email };
      const { exists } = await apiFetch<EmailCheckResult>("/auth/check-email", {
        method: "POST",
        body,
      });
      if (gen !== flowGen.current) return; // flow moved on — drop this result
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
      // The existence check is a UX nicety, not an auth precondition: on any
      // failure (network/400/429/5xx) degrade to the password step rather than
      // dead-ending both login and registration. The real check happens on submit.
      if (gen !== flowGen.current) return;
      setStep("password");
    } finally {
      if (gen === flowGen.current) setBusy(false);
    }
  }

  // Step 2 (login): sign in. Step 2 (register): advance to the interests step.
  async function onPasswordSubmit(e: FormEvent) {
    e.preventDefault();
    if (mode === "register") {
      setStep("interests");
      setError(null);
      return;
    }
    const gen = ++flowGen.current;
    setBusy(true);
    setError(null);
    try {
      await login(email, password);
      router.push(redirectTarget());
    } catch (err) {
      if (gen !== flowGen.current) return;
      if (handleRateLimit(err)) return;
      // Login bad-password is a 401 (code "unauthorized"); anything else is a
      // transient failure.
      setError(
        err instanceof ApiError && err.status === 401
          ? "Falsches Passwort."
          : "Anmeldung fehlgeschlagen — bitte erneut versuchen.",
      );
    } finally {
      if (gen === flowGen.current) setBusy(false);
    }
  }

  // Step 3 (register only): capture interests and create the account.
  async function onInterestsSubmit(e: FormEvent) {
    e.preventDefault();
    const gen = ++flowGen.current;
    setBusy(true);
    setError(null);
    try {
      const declared_categories = interests
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      await register({
        email,
        password,
        declared_categories,
        referral_code: refCode ?? undefined,
      });
      router.push(redirectTarget());
    } catch (err) {
      if (gen !== flowGen.current) return;
      if (handleRateLimit(err)) return;
      // Map on the backend's stable machine-readable code, not just HTTP status:
      // `email_taken` (409), `weak_password`/`invalid_email` (400).
      const code = err instanceof ApiError ? err.code : "";
      if (code === "invalid_referral_code") {
        // Drop the bad code so the NEXT attempt goes through without it.
        setRefCode(null);
        setError("Der Einladungscode ist ungültig — er wird ignoriert, versuch es erneut.");
      } else if (code === "email_taken") {
        setError("Diese E-Mail ist bereits registriert — wechsle zu „Anmelden“.");
      } else if (code === "weak_password") {
        setError("Passwort zu kurz (mindestens 8 Zeichen).");
      } else if (code === "invalid_email") {
        setError("Diese E-Mail-Adresse ist ungültig.");
      } else {
        setError("Registrierung fehlgeschlagen — bitte erneut versuchen.");
      }
    } finally {
      if (gen === flowGen.current) setBusy(false);
    }
  }

  // Ask the backend to email a one-time code, then move to the code step. Used
  // for both "forgot password" (reset) and "sign in with a code" (login). The
  // response is always 204 (no enumeration), so success here never confirms the
  // account exists — the code step is shown regardless.
  async function requestCode(which: Recovery) {
    const gen = ++flowGen.current;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const body: RequestCodeRequest = {
        email,
        purpose: which === "reset" ? "password_reset" : "login",
      };
      await apiFetch<void>("/auth/request-code", { method: "POST", body });
      if (gen !== flowGen.current) return;
      setRecovery(which);
      setCode("");
      setNewPassword("");
      setStep("code");
      setNotice(`Falls ein Konto für ${email} existiert, haben wir einen Code gesendet.`);
    } catch (err) {
      if (gen !== flowGen.current) return;
      if (handleRateLimit(err)) return;
      setError("Code konnte nicht angefordert werden — bitte erneut versuchen.");
    } finally {
      if (gen === flowGen.current) setBusy(false);
    }
  }

  // Code step: exchange the code for a session, or use it to set a new password.
  async function onCodeSubmit(e: FormEvent) {
    e.preventDefault();
    const gen = ++flowGen.current;
    setBusy(true);
    setError(null);
    try {
      if (recovery === "reset") {
        await resetPassword(email, code.trim(), newPassword);
      } else {
        await loginWithCode(email, code.trim());
      }
      router.push(redirectTarget());
    } catch (err) {
      if (gen !== flowGen.current) return;
      if (handleRateLimit(err)) return;
      const c = err instanceof ApiError ? err.code : "";
      if (recovery === "reset" && c === "weak_password") {
        setError("Neues Passwort zu kurz (mindestens 8 Zeichen).");
      } else if (err instanceof ApiError && err.status === 401) {
        setError("Der Code ist falsch oder abgelaufen.");
      } else {
        setError("Aktion fehlgeschlagen — bitte erneut versuchen.");
      }
    } finally {
      if (gen === flowGen.current) setBusy(false);
    }
  }

  const onSubmit =
    step === "email"
      ? onEmailSubmit
      : step === "password"
        ? onPasswordSubmit
        : step === "code"
          ? onCodeSubmit
          : onInterestsSubmit;

  const seg = (active: boolean) => ({
    flex: 1,
    padding: "9px 0",
    borderRadius: 999,
    border: "none",
    cursor: busy ? "default" : "pointer",
    fontSize: 14,
    fontWeight: active ? 600 : 500,
    color: active ? "#fff" : "rgba(255,255,255,0.55)",
    background: active ? "rgba(255,255,255,0.18)" : "transparent",
    opacity: busy && !active ? 0.5 : 1,
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
  const heading = { margin: 0, fontFamily: "var(--font-hanken), var(--font-inter), sans-serif", fontWeight: 700, fontSize: 26, color: "#fff", textAlign: "center" as const };
  const subtext = { margin: 0, fontSize: 14, color: "rgba(255,255,255,0.55)", textAlign: "center" as const };

  const primaryLabel =
    step === "email"
      ? "Weiter"
      : step === "interests"
        ? "Konto erstellen"
        : step === "code"
          ? recovery === "reset"
            ? "Passwort ändern"
            : "Anmelden"
          : mode === "login"
            ? "Anmelden"
            : "Weiter";
  const headingText =
    step === "code"
      ? recovery === "reset"
        ? "Neues Passwort"
        : "Per Code anmelden"
      : mode === "login"
        ? "Willkommen zurück"
        : "Konto erstellen";
  // Keep an accessible name on the busy button rather than a bare "…". During a
  // rate-limit cooldown the countdown replaces the label; the alert text stays
  // static so assistive tech hears one message, not a ticking number.
  const busyLabel = busy
    ? { children: "…", "aria-label": "Wird verarbeitet…" }
    : cooldownActive
      ? { children: `Warte ${cooldownLeft} s…` }
      : { children: primaryLabel };

  return (
    <div className="glass-login">
      <style>{`
        .glass-login { font-family: var(--font-inter), system-ui, -apple-system, sans-serif; }
        .glass-login input::placeholder { color: rgba(255,255,255,0.4); }
        .glass-login .gl-field:focus-within { border-color: rgba(255,255,255,0.42); background: rgba(255,255,255,0.11); }
        .glass-login .gl-primary:hover:not(:disabled) { background: rgba(255,255,255,0.30); }
        .glass-login .gl-primary:disabled { opacity: 0.6; cursor: default; }
        .glass-login .gl-back:hover:not(:disabled) { background: rgba(255,255,255,0.18); }
        .glass-login .gl-back:disabled { opacity: 0.5; cursor: default; }
        .glass-login .gl-change { background: none; border: none; padding: 0; cursor: pointer; color: rgba(255,255,255,0.85); text-decoration: underline; font: inherit; }
        .glass-login .gl-change:disabled { cursor: default; opacity: 0.6; }
        .glass-login .gl-legal { color: rgba(255,255,255,0.62); font-weight: 500; text-decoration: none; }
        .glass-login .gl-legal:hover { text-decoration: underline; }
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
          disabled={busy}
          onClick={() => {
            if (step === "interests") backToPassword();
            else if (step === "code") backToPassword();
            else if (step === "password") backToEmail();
            // Fresh tab has no history — router.back() is a no-op there, so fall
            // back to a real destination.
            else if (window.history.length > 1) router.back();
            else router.push("/");
          }}
          aria-label="Zurück"
          style={{ position: "absolute", top: 24, left: 24, width: 36, height: 36, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 999, background: "rgba(255,255,255,0.10)", border: "1px solid rgba(255,255,255,0.22)", color: "rgba(255,255,255,0.85)", cursor: "pointer", transition: "background 0.15s" }}
        >
          <BackIcon />
        </button>

        <form
          onSubmit={onSubmit}
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
          <div style={{ display: "flex", alignItems: "center", justifyContent: "center", paddingBottom: 6 }}>
            <img src="/logo.svg" alt="Poolsite" style={{ height: 76, width: "auto", display: "block" }} />
          </div>

          {/* Anmelden / Registrieren tabs */}
          <div role="tablist" aria-label="Anmelden oder Registrieren" style={{ display: "flex", gap: 4, width: "100%", padding: 4, borderRadius: 999, background: "rgba(255,255,255,0.07)", boxSizing: "border-box" }}>
            <button type="button" role="tab" aria-selected={mode === "login"} disabled={busy} style={seg(mode === "login")} onClick={() => switchMode("login")}>
              Anmelden
            </button>
            <button type="button" role="tab" aria-selected={mode === "register"} disabled={busy} style={seg(mode === "register")} onClick={() => switchMode("register")}>
              Registrieren
            </button>
          </div>

          <h1 style={heading}>{headingText}</h1>
          <p style={subtext}>
            {step === "email" ? (
              mode === "login" ? "Gib deine E-Mail ein, um dich anzumelden." : "Gib deine E-Mail ein, um zu starten."
            ) : step === "interests" ? (
              "Wähle ein paar Interessen (optional) — das verbessert deinen Feed."
            ) : step === "code" ? (
              recovery === "reset"
                ? "Gib den Code aus der E-Mail ein und wähle ein neues Passwort."
                : "Gib den Code aus der E-Mail ein, um dich anzumelden."
            ) : (
              <>
                {email}
                {" · "}
                <button type="button" className="gl-change" disabled={busy} onClick={backToEmail}>
                  Ändern
                </button>
              </>
            )}
          </p>

          {step === "email" && (
            <div className="gl-field" style={field}>
              <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }} aria-hidden>
                <MailIcon />
              </span>
              <input
                type="email"
                required
                autoFocus
                autoComplete="email"
                aria-label="E-Mail-Adresse"
                placeholder="du@email.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                style={input}
              />
            </div>
          )}

          {step === "password" && (
            <div className="gl-field" style={field}>
              <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }} aria-hidden>
                <LockIcon />
              </span>
              <input
                type="password"
                required
                autoFocus
                minLength={mode === "register" ? 8 : undefined}
                autoComplete={mode === "login" ? "current-password" : "new-password"}
                aria-label="Passwort"
                placeholder={mode === "login" ? "Passwort" : "Passwort wählen (min. 8 Zeichen)"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                style={input}
              />
            </div>
          )}

          {step === "interests" && (
            <div className="gl-field" style={field}>
              <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }} aria-hidden>
                <TagIcon />
              </span>
              <input
                type="text"
                autoFocus
                aria-label="Interessen (kommagetrennt, optional)"
                placeholder="Musik, Tech, Kunst…"
                value={interests}
                onChange={(e) => setInterests(e.target.value)}
                style={input}
              />
            </div>
          )}

          {step === "code" && (
            <>
              <div className="gl-field" style={field}>
                <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }} aria-hidden>
                  <LockIcon />
                </span>
                <input
                  type="text"
                  required
                  autoFocus
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  pattern="[0-9]*"
                  maxLength={6}
                  aria-label="Code aus der E-Mail"
                  placeholder="6-stelliger Code"
                  value={code}
                  onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
                  style={{ ...input, letterSpacing: "0.3em" }}
                />
              </div>
              {recovery === "reset" && (
                <div className="gl-field" style={field}>
                  <span style={{ color: "rgba(255,255,255,0.6)", display: "flex" }} aria-hidden>
                    <LockIcon />
                  </span>
                  <input
                    type="password"
                    required
                    minLength={8}
                    autoComplete="new-password"
                    aria-label="Neues Passwort"
                    placeholder="Neues Passwort (min. 8 Zeichen)"
                    value={newPassword}
                    onChange={(e) => setNewPassword(e.target.value)}
                    style={input}
                  />
                </div>
              )}
              <button
                type="button"
                className="gl-change"
                disabled={busy || cooldownActive}
                onClick={() => requestCode(recovery)}
                style={{ fontSize: 13 }}
              >
                Code erneut senden
              </button>
            </>
          )}

          {notice && !error && (
            <p aria-live="polite" style={{ margin: 0, width: "100%", fontSize: 13, color: "rgba(255,255,255,0.7)", textAlign: "center" }}>
              {notice}
            </p>
          )}

          {error && (
            <p role="alert" aria-live="assertive" style={{ margin: 0, width: "100%", fontSize: 13, color: "#ff8585", textAlign: "center" }}>
              {error}
            </p>
          )}

          <button
            type="submit"
            className="gl-primary"
            disabled={busy || cooldownActive}
            style={{ width: "100%", padding: "15px 0", borderRadius: 999, background: "rgba(255,255,255,0.22)", border: "1.2px solid rgba(255,255,255,0.42)", boxShadow: "0 8px 20px rgba(0,0,0,0.4)", color: "#fff", fontSize: 15, fontWeight: 500, cursor: "pointer", transition: "background 0.15s" }}
            {...busyLabel}
          />

          {step === "password" && mode === "login" && (
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 8, fontSize: 13 }}>
              <button type="button" className="gl-change" disabled={busy || cooldownActive} onClick={() => requestCode("reset")}>
                Passwort vergessen?
              </button>
              <button type="button" className="gl-change" disabled={busy || cooldownActive} onClick={() => requestCode("login")}>
                Per E-Mail-Code anmelden
              </button>
            </div>
          )}

          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, paddingTop: 6, fontSize: 11 }}>
            <span style={{ color: "rgba(255,255,255,0.38)", textAlign: "center" }}>Mit Fortfahren akzeptierst du unsere</span>
            <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <a className="gl-legal" href="/legal/agb">AGB</a>
              <span style={{ color: "rgba(255,255,255,0.3)" }}>·</span>
              <a className="gl-legal" href="/legal/datenschutz">Datenschutz</a>
            </span>
          </div>
        </form>
      </div>
    </div>
  );
}
