import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthProvider, useAuth } from "./auth";

const TOKEN_KEY = "gamma_token";

function Probe() {
  const { token, ready } = useAuth();
  return <div data-testid="probe">{ready ? `ready:${token ?? "null"}` : "loading"}</div>;
}

// Mocks GET /auth/me with the given status (200 returns a user; otherwise an error).
function mockMe(status: number) {
  return vi.fn().mockResolvedValue({
    ok: status >= 200 && status < 300,
    status,
    statusText: "Status",
    json: async () => (status === 200 ? { user_id: 1, role: "user" } : { error: "x" }),
  } as Response);
}

function renderProvider() {
  return render(
    <AuthProvider>
      <Probe />
    </AuthProvider>,
  );
}

describe("AuthProvider session restore", () => {
  beforeEach(() => sessionStorage.clear());
  afterEach(() => vi.unstubAllGlobals());

  it("keeps a valid token when /auth/me fails transiently (5xx, not a 401)", async () => {
    sessionStorage.setItem(TOKEN_KEY, "valid-token");
    vi.stubGlobal("fetch", mockMe(500));

    renderProvider();
    await screen.findByText(/^ready:/);

    // A transient blip must NOT log the user out.
    expect(sessionStorage.getItem(TOKEN_KEY)).toBe("valid-token");
  });

  it("evicts the token when /auth/me returns 401", async () => {
    sessionStorage.setItem(TOKEN_KEY, "stale-token");
    vi.stubGlobal("fetch", mockMe(401));

    renderProvider();
    await screen.findByText(/^ready:/);

    expect(sessionStorage.getItem(TOKEN_KEY)).toBeNull();
  });

  it("logs out globally when a gamma:unauthorized event fires", async () => {
    sessionStorage.setItem(TOKEN_KEY, "tok");
    vi.stubGlobal("fetch", mockMe(200));

    renderProvider();
    await screen.findByText("ready:tok"); // session restored

    act(() => {
      window.dispatchEvent(new Event("gamma:unauthorized"));
    });

    await waitFor(() => expect(sessionStorage.getItem(TOKEN_KEY)).toBeNull());
    await screen.findByText("ready:null");
  });
});
