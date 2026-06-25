// Typed fetch wrapper over the core API. Every request goes through here so auth,
// error handling, and the base URL live in one place.
//
// Bigint note: the generated contract maps i64/u64 to `bigint`, but `JSON.parse`
// yields `number` at runtime. We cast the parsed body to the contract type and use
// `String(x)` for display (works for both). Request bodies carry no bigint fields.

import { API_BASE_URL } from "./config";

/** A non-2xx API response. `code` is the backend's stable machine-readable error
 * code (the `{ "error": code }` body), or the HTTP status text as a fallback. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(`API ${status}: ${code}`);
    this.name = "ApiError";
  }
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
    throw new ApiError(resp.status, code);
  }

  if (resp.status === 204) return undefined as T;
  return (await resp.json()) as T;
}
