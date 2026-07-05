import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "@/lib/api";

// Hoisted mocks: the router, the check-email fetch, and the auth login/register.
const { pushMock, apiFetchMock, loginMock, registerMock, searchParamsRef } = vi.hoisted(() => ({
  pushMock: vi.fn(),
  apiFetchMock: vi.fn(),
  loginMock: vi.fn(),
  registerMock: vi.fn(),
  searchParamsRef: { current: new URLSearchParams() },
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock, replace: vi.fn(), back: vi.fn() }),
  useSearchParams: () => searchParamsRef.current,
}));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  apiFetch: apiFetchMock,
}));
vi.mock("@/lib/auth", () => ({
  useAuth: () => ({ login: loginMock, register: registerMock }),
}));

import LoginPage from "./page";

// The check-email endpoint resolves { exists } based on the request body's email.
function mockCheckEmail(exists: boolean) {
  apiFetchMock.mockImplementation((path: string) => {
    if (path === "/auth/check-email") return Promise.resolve({ exists });
    return Promise.resolve(undefined);
  });
}

function typeEmail(value: string) {
  fireEvent.change(screen.getByLabelText("E-Mail-Adresse"), { target: { value } });
}

beforeEach(() => {
  pushMock.mockReset();
  apiFetchMock.mockReset();
  loginMock.mockReset();
  registerMock.mockReset();
  searchParamsRef.current = new URLSearchParams();
});
afterEach(() => cleanup());

describe("LoginPage", () => {
  it("advances email → password when the account exists (login)", async () => {
    mockCheckEmail(true);
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    // The password field appears once the check resolves.
    await screen.findByLabelText("Passwort");
    expect(screen.queryByLabelText("E-Mail-Adresse")).toBeNull();
  });

  it("blocks login for an unknown email and hints to register", async () => {
    mockCheckEmail(false);
    render(<LoginPage />);

    typeEmail("nobody@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toMatch(/Registrieren/);
    // Still on the email step — never advanced to a password.
    expect(screen.queryByLabelText("Passwort")).toBeNull();
  });

  it("blocks register when the email is already taken and hints to sign in", async () => {
    mockCheckEmail(true);
    render(<LoginPage />);

    fireEvent.click(screen.getByRole("tab", { name: "Registrieren" }));
    typeEmail("taken@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toMatch(/Anmelden/);
  });

  it("degrades to the password step when check-email fails (finding 2)", async () => {
    // Any failure (network/5xx/429) must NOT dead-end — it falls through to password.
    apiFetchMock.mockImplementation((path: string) => {
      if (path === "/auth/check-email") return Promise.reject(new ApiError(500, "internal_error"));
      return Promise.resolve(undefined);
    });
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    await screen.findByLabelText("Passwort");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("maps a wrong-password 401 to the correct message on login", async () => {
    mockCheckEmail(true);
    loginMock.mockRejectedValue(new ApiError(401, "unauthorized"));
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
    const pw = await screen.findByLabelText("Passwort");

    fireEvent.change(pw, { target: { value: "wrongpass" } });
    fireEvent.click(screen.getByRole("button", { name: "Anmelden" }));

    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toBe("Falsches Passwort.");
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("redirects to ?next= after a successful login", async () => {
    searchParamsRef.current = new URLSearchParams("next=/posts/7");
    mockCheckEmail(true);
    loginMock.mockResolvedValue(undefined);
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
    const pw = await screen.findByLabelText("Passwort");
    fireEvent.change(pw, { target: { value: "correcthorse" } });
    fireEvent.click(screen.getByRole("button", { name: "Anmelden" }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith("/posts/7"));
  });

  it("captures interests and maps email_taken via ApiError.code on register", async () => {
    mockCheckEmail(false);
    registerMock.mockRejectedValue(new ApiError(409, "email_taken"));
    render(<LoginPage />);

    fireEvent.click(screen.getByRole("tab", { name: "Registrieren" }));
    typeEmail("new@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    const pw = await screen.findByLabelText("Passwort");
    fireEvent.change(pw, { target: { value: "longenoughpw" } });
    // register step 2 advances to the interests step, not a direct submit.
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    const interests = await screen.findByLabelText(/Interessen/);
    fireEvent.change(interests, { target: { value: "music, tech" } });
    fireEvent.click(screen.getByRole("button", { name: "Konto erstellen" }));

    await screen.findByRole("alert");
    // Mapped on the stable code, not the raw status.
    expect(screen.getByRole("alert").textContent).toMatch(/bereits registriert/);
    expect(registerMock).toHaveBeenCalledWith({
      email: "new@example.com",
      password: "longenoughpw",
      declared_categories: ["music", "tech"],
    });
  });

  it("shows a static alert + countdown button on a 429 login, surviving a tab switch", async () => {
    mockCheckEmail(true);
    // No retryAfter on the error → the UI falls back to a 30s cooldown.
    loginMock.mockRejectedValue(new ApiError(429, "rate_limited"));
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
    const pw = await screen.findByLabelText("Passwort");
    fireEvent.change(pw, { target: { value: "whatever42" } });
    fireEvent.click(screen.getByRole("button", { name: "Anmelden" }));

    await screen.findByRole("alert");
    expect(screen.getByRole("alert").textContent).toBe("Zu viele Versuche — bitte warte kurz.");
    const btn = screen.getByRole("button", { name: /Warte \d+ s/ }) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(pushMock).not.toHaveBeenCalled();

    // The cooldown deliberately survives a tab switch — the server-side limit
    // is real regardless of which tab is showing.
    fireEvent.click(screen.getByRole("tab", { name: "Registrieren" }));
    const after = screen.getByRole("button", { name: /Warte \d+ s/ }) as HTMLButtonElement;
    expect(after.disabled).toBe(true);
  });

  it("re-enables submit once the Retry-After cooldown has run out", async () => {
    mockCheckEmail(true);
    loginMock.mockRejectedValue(new ApiError(429, "rate_limited", 1));
    render(<LoginPage />);

    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
    const pw = await screen.findByLabelText("Passwort");
    fireEvent.change(pw, { target: { value: "whatever42" } });
    fireEvent.click(screen.getByRole("button", { name: "Anmelden" }));

    await screen.findByRole("alert");
    // After the 1s cooldown the normal label returns and the button is usable.
    await waitFor(
      () => {
        const btn = screen.getByRole("button", { name: "Anmelden" }) as HTMLButtonElement;
        expect(btn.disabled).toBe(false);
      },
      { timeout: 3000 },
    );
  });

  it("does not corrupt state when the tab is switched mid-flow (regression, finding 1)", async () => {
    // A deferred check-email lets us switch tabs while the request is in flight.
    let resolveCheck: ((v: { exists: boolean }) => void) | null = null;
    apiFetchMock.mockImplementation((path: string) => {
      if (path === "/auth/check-email") {
        return new Promise<{ exists: boolean }>((res) => {
          resolveCheck = res;
        });
      }
      return Promise.resolve(undefined);
    });
    render(<LoginPage />);

    // Start a LOGIN check for a known account…
    typeEmail("me@example.com");
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));

    // …but switch to Registrieren before it resolves. The tab is disabled while
    // busy, so first the in-flight request must settle. Resolve it now with a
    // stale result (exists:true) that belongs to the LOGIN flow.
    await waitFor(() => expect(resolveCheck).not.toBeNull());
    resolveCheck!({ exists: true });

    // login's exists:true would have advanced to password; assert it did, THEN
    // switching tabs resets cleanly back to the email step (no corruption).
    await screen.findByLabelText("Passwort");
    // The busy state has cleared, so the tab is clickable again.
    await waitFor(() =>
      expect((screen.getByRole("tab", { name: "Registrieren" }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
    fireEvent.click(screen.getByRole("tab", { name: "Registrieren" }));

    // Back to a clean email step for the new mode — no leftover password field.
    await screen.findByLabelText("E-Mail-Adresse");
    expect(screen.queryByLabelText("Passwort")).toBeNull();
    expect(screen.getByRole("tab", { name: "Registrieren" }).getAttribute("aria-selected")).toBe(
      "true",
    );
  });
});
