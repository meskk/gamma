import { renderHook, waitFor } from "@testing-library/react";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";

import { useFetch } from "./useFetch";

describe("useFetch", () => {
  it("resolves data and clears loading", async () => {
    const { result } = renderHook(() => useFetch(() => Promise.resolve(42), []));
    expect(result.current.loading).toBe(true);
    await waitFor(() => expect(result.current.data).toBe(42));
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("drops out-of-order responses (the stale-guard, done once)", async () => {
    let resolveSlow: ((v: string) => void) | null = null;
    const slow = new Promise<string>((res) => {
      resolveSlow = res;
    });
    const fetcher = vi
      .fn()
      .mockImplementationOnce(() => slow)
      .mockImplementationOnce(() => Promise.resolve("fresh"));

    const { result, rerender } = renderHook(({ id }) => useFetch(fetcher, [id]), {
      initialProps: { id: 1 },
    });
    // Navigate before the first response lands…
    rerender({ id: 2 });
    await waitFor(() => expect(result.current.data).toBe("fresh"));
    // …then the SLOW response arrives and must NOT overwrite the fresh one.
    await act(async () => resolveSlow!("stale"));
    expect(result.current.data).toBe("fresh");
  });

  it("resets data on dep change (no stale flash) and surfaces the raw error", async () => {
    const boom = new Error("boom");
    const fetcher = vi
      .fn()
      .mockImplementationOnce(() => Promise.resolve("one"))
      .mockImplementationOnce(() => Promise.reject(boom));

    const { result, rerender } = renderHook(({ id }) => useFetch(fetcher, [id]), {
      initialProps: { id: 1 },
    });
    await waitFor(() => expect(result.current.data).toBe("one"));

    rerender({ id: 2 });
    expect(result.current.data).toBeNull(); // reset immediately, no flash
    await waitFor(() => expect(result.current.error).toBe(boom));
    expect(result.current.data).toBeNull();
  });

  it("reload refetches in place", async () => {
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce("v1")
      .mockResolvedValueOnce("v2");
    const { result } = renderHook(() => useFetch(fetcher, []));
    await waitFor(() => expect(result.current.data).toBe("v1"));
    await act(async () => result.current.reload());
    await waitFor(() => expect(result.current.data).toBe("v2"));
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("enabled: false idles without fetching, then fetches once enabled", async () => {
    const fetcher = vi.fn().mockResolvedValue("ready");
    const { result, rerender } = renderHook(
      ({ on }) => useFetch(fetcher, [], { enabled: on }),
      { initialProps: { on: false } },
    );
    expect(result.current.loading).toBe(false);
    expect(fetcher).not.toHaveBeenCalled();

    rerender({ on: true });
    await waitFor(() => expect(result.current.data).toBe("ready"));
  });
});
