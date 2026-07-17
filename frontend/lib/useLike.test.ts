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
  it("hydrates from the server row and toggles like → POST, unlike → DELETE", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useLike({ postId: "7" }, "tok", { liked: false, count: 3 }),
    );
    expect(result.current.liked).toBe(false);
    expect(result.current.count).toBe(3);

    // Like: optimistic flip + count bump, POST body carries the post target.
    await act(async () => result.current.toggle());
    expect(result.current.liked).toBe(true);
    expect(result.current.count).toBe(4);
    expect(apiFetchMock).toHaveBeenCalledWith("/interactions", {
      method: "POST",
      body: { type: "like", target_id: null, post_id: 7, comment_id: null },
      token: "tok",
    });

    // Unlike: back to the server baseline, via DELETE.
    await act(async () => result.current.toggle());
    expect(result.current.liked).toBe(false);
    expect(result.current.count).toBe(3);
    expect(apiFetchMock).toHaveBeenLastCalledWith("/interactions", {
      method: "DELETE",
      body: { type: "like", target_id: null, post_id: 7, comment_id: null },
      token: "tok",
    });
  });

  it("starts liked when the server says so, and unliking decrements", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useLike({ postId: "7" }, "tok", { liked: true, count: 5 }),
    );
    expect(result.current.liked).toBe(true);
    expect(result.current.count).toBe(5);

    await act(async () => result.current.toggle());
    expect(result.current.liked).toBe(false);
    expect(result.current.count).toBe(4);
  });

  it("targets a comment when commentId is given", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useLike({ commentId: "12" }, "tok", { liked: false, count: 0 }),
    );
    await act(async () => result.current.toggle());
    expect(apiFetchMock).toHaveBeenCalledWith("/interactions", {
      method: "POST",
      body: { type: "like", target_id: null, post_id: null, comment_id: 12 },
      token: "tok",
    });
  });

  it("reverts on failure", async () => {
    apiFetchMock.mockImplementation(() => Promise.reject(new Error("down")));
    const { result } = renderHook(() =>
      useLike({ postId: "7" }, "tok", { liked: false, count: 3 }),
    );

    await act(async () => result.current.toggle());
    await waitFor(() => expect(result.current.liked).toBe(false));
    expect(result.current.count).toBe(3);
  });

  it("resets the local override when navigating to another post", async () => {
    apiFetchMock.mockResolvedValue(undefined);
    const { result, rerender } = renderHook(
      ({ id }) => useLike({ postId: id }, "tok", { liked: false, count: 0 }),
      { initialProps: { id: "7" } },
    );
    await act(async () => result.current.toggle());
    expect(result.current.liked).toBe(true);

    rerender({ id: "8" });
    await waitFor(() => expect(result.current.liked).toBe(false));
  });

  it("does not write a failure-revert into the NEXT target after navigation", async () => {
    let reject!: (e: Error) => void;
    apiFetchMock.mockImplementation(
      () =>
        new Promise<void>((_, rej) => {
          reject = rej;
        }),
    );
    const { result, rerender } = renderHook(
      ({ id }) => useLike({ postId: id }, "tok", { liked: true, count: 1 }),
      { initialProps: { id: "7" } },
    );
    // Unlike post 7 — the DELETE hangs.
    act(() => {
      void result.current.toggle();
    });
    expect(result.current.liked).toBe(false);

    // Navigate to post 8, which the server also reports as liked.
    rerender({ id: "8" });
    await waitFor(() => expect(result.current.liked).toBe(true));

    // Post 7's request now fails: the revert must NOT flip post 8's state.
    await act(async () => {
      reject(new Error("down"));
    });
    expect(result.current.liked).toBe(true);
  });

  it("is single-flight: a second toggle during an in-flight request is ignored", async () => {
    let release!: () => void;
    apiFetchMock.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { result } = renderHook(() =>
      useLike({ postId: "7" }, "tok", { liked: false, count: 0 }),
    );

    act(() => {
      void result.current.toggle();
    });
    expect(result.current.liked).toBe(true);
    // Second click while the POST is still in flight: no second request.
    await act(async () => result.current.toggle());
    expect(apiFetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      release();
    });
    expect(result.current.liked).toBe(true);
  });
});
