import { renderHook, waitFor } from "@testing-library/react";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { apiFetchMock } = vi.hoisted(() => ({ apiFetchMock: vi.fn() }));
vi.mock("@/lib/api", () => ({ apiFetch: apiFetchMock }));

import { useUnlock } from "./useUnlock";

const locked = { kind: "video", unlock_price: 25, playback_url: null };
const open = { kind: "video", unlock_price: 25, playback_url: "https://cdn/x.mp4" };

beforeEach(() => {
  // Block body ON PURPOSE: mockReset() returns the mock, and a function
  // returned from a vitest hook is invoked as a TEARDOWN callback — which
  // would call the mock bare and leak an unhandled rejection.
  apiFetchMock.mockReset();
});

describe("useUnlock", () => {
  it("loads the view, unlocks, and refetches to the entitled view", async () => {
    apiFetchMock.mockImplementation((path?: string, opts?: { method?: string }) => {
      if (path === "/media/5/unlock" && opts?.method === "POST") return Promise.resolve(undefined);
      if (path === "/media/5") {
        // Locked before the unlock POST happened, open afterwards.
        const unlocked = apiFetchMock.mock.calls.some((c) => c[0] === "/media/5/unlock");
        return Promise.resolve(unlocked ? open : locked);
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useUnlock("5", "tok"));
    await waitFor(() => expect(result.current.view).toEqual(locked));

    await act(async () => result.current.unlock());
    await waitFor(() => expect(result.current.view).toEqual(open));
    expect(result.current.unlockError).toBe(false);
  });

  it("keeps the locked view and flags unlockError on failure", async () => {
    apiFetchMock.mockImplementation((path?: string, opts?: { method?: string }) =>
      opts?.method === "POST" ? Promise.reject(new Error("broke")) : Promise.resolve(locked),
    );

    const { result } = renderHook(() => useUnlock("5", "tok"));
    await waitFor(() => expect(result.current.view).toEqual(locked));

    await act(async () => result.current.unlock());
    await waitFor(() => expect(result.current.unlockError).toBe(true));
    expect(result.current.view).toEqual(locked);
  });

  it("stays idle until enabled (the reel track's lazy load)", async () => {
    apiFetchMock.mockResolvedValue(locked);
    const { result, rerender } = renderHook(({ on }) => useUnlock("5", "tok", { enabled: on }), {
      initialProps: { on: false },
    });
    expect(apiFetchMock).not.toHaveBeenCalled();
    expect(result.current.view).toBeNull();

    rerender({ on: true });
    await waitFor(() => expect(result.current.view).toEqual(locked));
  });
});
