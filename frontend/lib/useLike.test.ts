import { renderHook, waitFor } from "@testing-library/react";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { apiFetchMock } = vi.hoisted(() => ({ apiFetchMock: vi.fn() }));
vi.mock("@/lib/api", () => ({ apiFetch: apiFetchMock }));

import { useLike } from "./useLike";

beforeEach(() => {
  // Block body ON PURPOSE: mockReset() returns the mock, and a function
  // returned from a vitest hook is invoked as a TEARDOWN callback — which
  // would call the mock bare and leak an unhandled rejection.
  apiFetchMock.mockReset();
});

describe("useLike", () => {
  it("sets optimistically, posts once, and stays liked (single-shot)", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result } = renderHook(() => useLike("7", "tok"));

    await act(async () => result.current.like());
    expect(result.current.liked).toBe(true);
    expect(apiFetchMock).toHaveBeenCalledWith("/interactions", {
      method: "POST",
      body: { type: "like", target_id: null, post_id: 7 },
      token: "tok",
    });

    await act(async () => result.current.like());
    expect(apiFetchMock).toHaveBeenCalledTimes(1);
  });

  it("reverts on failure", async () => {
    apiFetchMock.mockImplementation(() => Promise.reject(new Error("down")));
    const { result } = renderHook(() => useLike("7", "tok"));

    await act(async () => result.current.like());
    await waitFor(() => expect(result.current.liked).toBe(false));
  });

  it("resets when navigating to another post", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result, rerender } = renderHook(({ id }) => useLike(id, "tok"), {
      initialProps: { id: "7" },
    });
    await act(async () => result.current.like());
    expect(result.current.liked).toBe(true);

    rerender({ id: "8" });
    await waitFor(() => expect(result.current.liked).toBe(false));
  });
});
