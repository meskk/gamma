// Typed fetch wrapper over the core API. Every request goes through here so auth,
// error handling, and the base URL live in one place.
//
// Bigint note: the generated contract maps i64/u64 to `bigint`, but `JSON.parse`
// yields `number` at runtime. We cast the parsed body to the contract type and use
// `String(x)` for display (works for both). Request bodies carry no bigint fields.

import { API_BASE_URL } from "./config";

/** A non-2xx API response. `code` is the backend's stable machine-readable error
 * code (the `{ "error": code }` body), or the HTTP status text as a fallback.
 * `retryAfter` carries the parsed `Retry-After` header in seconds (rate-limit
 * 429s), or `null` when absent/unparseable — so UIs can render a countdown. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly retryAfter: number | null = null,
  ) {
    super(`API ${status}: ${code}`);
    this.name = "ApiError";
  }
}

/** Parse a `Retry-After` header value: delta-seconds or an HTTP-date, clamped
 * to >= 0; `null` for garbage. Capped at 15 minutes so a bogus header can't
 * freeze a UI for hours. */
function parseRetryAfter(value: string | null): number | null {
  if (!value) return null;
  const capped = (secs: number) => Math.min(Math.max(Math.ceil(secs), 0), 15 * 60);
  if (/^\d+$/.test(value.trim())) return capped(Number(value));
  const asDate = Date.parse(value);
  if (!Number.isNaN(asDate)) return capped((asDate - Date.now()) / 1000);
  return null;
}

type RequestOptions = {
  method?: "GET" | "POST" | "PUT" | "DELETE";
  body?: unknown;
  token?: string | null;
};

export async function apiFetch<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (opts.body !== undefined) headers["Content-Type"] = "application/json";
  if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

  const resp = await fetch(`${API_BASE_URL}${path}`, {
    method: opts.method ?? "GET",
    headers,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
  });

  if (!resp.ok) {
    let code = resp.statusText;
    try {
      const body = (await resp.json()) as { error?: unknown };
      if (typeof body?.error === "string") code = body.error;
    } catch {
      // Non-JSON error body — keep the status text.
    }
    // A 401 on an AUTHENTICATED request (one that carried a token) means the
    // session is no longer valid — signal "logged out" globally. A 401 without
    // a token (e.g. a bad-password login) is just a failed attempt, not an
    // eviction. Use a browser event to avoid SSR/module-singleton coupling.
    if (resp.status === 401 && opts.token && typeof window !== "undefined") {
      window.dispatchEvent(new Event("gamma:unauthorized"));
    }
    throw new ApiError(resp.status, code, parseRetryAfter(resp.headers.get("retry-after")));
  }

  if (resp.status === 204) return undefined as T;
  return (await resp.json()) as T;
}
