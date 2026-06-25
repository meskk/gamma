import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// config.ts reads NEXT_PUBLIC_API_BASE_URL at module load, so each case stubs the
// env and re-imports the module fresh.
describe("API_BASE_URL", () => {
  beforeEach(() => vi.resetModules());
  afterEach(() => vi.unstubAllEnvs());

  it("falls back to the localhost default when the env var is an empty string", async () => {
    vi.stubEnv("NEXT_PUBLIC_API_BASE_URL", "");
    const { API_BASE_URL } = await import("./config");
    expect(API_BASE_URL).toBe("http://localhost:8080/v1");
  });

  it("uses the env var when it is set", async () => {
    vi.stubEnv("NEXT_PUBLIC_API_BASE_URL", "https://api.example.com/v1");
    const { API_BASE_URL } = await import("./config");
    expect(API_BASE_URL).toBe("https://api.example.com/v1");
  });
});
