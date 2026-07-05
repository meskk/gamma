import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, apiFetch } from "./api";

function mockFetch(opts: {
  status: number;
  jsonBody?: unknown;
  headers?: Record<string, string>;
}) {
  const ok = opts.status >= 200 && opts.status < 300;
  return vi.fn().mockResolvedValue({
    ok,
    status: opts.status,
    statusText: "Status",
    headers: new Headers(opts.headers),
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

  it("parses Retry-After delta-seconds on a 429", async () => {
    vi.stubGlobal(
      "fetch",
      mockFetch({
        status: 429,
        jsonBody: { error: "rate_limited" },
        headers: { "retry-after": "30" },
      }),
    );
    await expect(apiFetch("/auth/login")).rejects.toMatchObject({
      status: 429,
      code: "rate_limited",
      retryAfter: 30,
    });
  });

  it("parses an HTTP-date Retry-After and caps bogus values at 15 minutes", async () => {
    const inAMinute = new Date(Date.now() + 60_000).toUTCString();
    vi.stubGlobal(
      "fetch",
      mockFetch({ status: 429, headers: { "retry-after": inAMinute } }),
    );
    const err = (await apiFetch("/x").catch((e) => e)) as ApiError;
    expect(err.retryAfter).toBeGreaterThan(0);
    expect(err.retryAfter).toBeLessThanOrEqual(60);

    vi.stubGlobal(
      "fetch",
      mockFetch({ status: 429, headers: { "retry-after": "99999" } }),
    );
    const capped = (await apiFetch("/x").catch((e) => e)) as ApiError;
    expect(capped.retryAfter).toBe(15 * 60);
  });

  it("yields retryAfter null when the header is absent or garbage", async () => {
    vi.stubGlobal("fetch", mockFetch({ status: 429 }));
    const absent = (await apiFetch("/x").catch((e) => e)) as ApiError;
    expect(absent.retryAfter).toBeNull();

    vi.stubGlobal(
      "fetch",
      mockFetch({ status: 429, headers: { "retry-after": "soonish" } }),
    );
    const garbage = (await apiFetch("/x").catch((e) => e)) as ApiError;
    expect(garbage.retryAfter).toBeNull();
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
