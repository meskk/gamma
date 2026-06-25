import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, apiFetch } from "./api";

function mockFetch(opts: { status: number; jsonBody?: unknown }) {
  const ok = opts.status >= 200 && opts.status < 300;
  return vi.fn().mockResolvedValue({
    ok,
    status: opts.status,
    statusText: "Status",
    json: async () => opts.jsonBody ?? {},
  } as Response);
}

describe("apiFetch", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("returns the parsed body on 200", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 200, jsonBody: { hello: "world" } }));
    await expect(apiFetch("/x")).resolves.toEqual({ hello: "world" });
  });

  it("returns undefined on 204 (no body parse)", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 204 }));
    await expect(apiFetch("/x")).resolves.toBeUndefined();
  });

  it("throws ApiError carrying the backend error code on non-2xx", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 400, jsonBody: { error: "empty_body" } }));
    await expect(apiFetch("/x")).rejects.toMatchObject({ status: 400, code: "empty_body" });
  });

  it("dispatches gamma:unauthorized on a 401 for an authenticated request", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 401, jsonBody: { error: "unauthorized" } }));
    const onUnauthorized = vi.fn();
    window.addEventListener("gamma:unauthorized", onUnauthorized);
    await expect(apiFetch("/me", { token: "tok" })).rejects.toBeInstanceOf(ApiError);
    expect(onUnauthorized).toHaveBeenCalledOnce();
    window.removeEventListener("gamma:unauthorized", onUnauthorized);
  });

  it("does NOT dispatch on a 401 without a token (e.g. a failed login)", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 401, jsonBody: { error: "invalid_credentials" } }));
    const onUnauthorized = vi.fn();
    window.addEventListener("gamma:unauthorized", onUnauthorized);
    await expect(apiFetch("/auth/login", { method: "POST", body: {} })).rejects.toBeInstanceOf(
      ApiError,
    );
    expect(onUnauthorized).not.toHaveBeenCalled();
    window.removeEventListener("gamma:unauthorized", onUnauthorized);
  });
});
