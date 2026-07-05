import { renderHook, waitFor } from "@testing-library/react";
import { act } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { apiFetchMock } = vi.hoisted(() => ({ apiFetchMock: vi.fn() }));
vi.mock("@/lib/api", () => ({ apiFetch: apiFetchMock }));

import { usePagedFeed } from "./usePagedFeed";

const post = (id: number) => ({ id, author_id: 1, body: `p${id}` });

afterEach(() => {
  // Block body ON PURPOSE — see useLike.test.ts: a returned mock would be
  // invoked as a teardown callback.
  apiFetchMock.mockReset();
});

describe("usePagedFeed", () => {
  it("loads page one and appends deduped pages until the cursor runs out", async () => {
    apiFetchMock
      .mockResolvedValueOnce({ items: [post(1), post(2)], next_cursor: "c1" })
      // Page two overlaps (cursor drift) — the dupe must be dropped.
      .mockResolvedValueOnce({ items: [post(2), post(3)], next_cursor: null });

    const { result } = renderHook(() => usePagedFeed("7", "tok"));
    await waitFor(() => expect(result.current.posts).toHaveLength(2));

    await act(async () => result.current.loadMore());
    await waitFor(() => expect(result.current.posts).toHaveLength(3));
    expect(result.current.posts!.map((p) => Number(p.id))).toEqual([1, 2, 3]);

    // Cursor exhausted: further calls never hit the network.
    await act(async () => result.current.loadMore());
    expect(apiFetchMock).toHaveBeenCalledTimes(2);
  });

  it("treats a legacy bare-array response as a single page", async () => {
    apiFetchMock.mockResolvedValueOnce([post(1), post(2), post(3)]);

    const { result } = renderHook(() => usePagedFeed("7", "tok"));
    await waitFor(() => expect(result.current.posts).toHaveLength(3));

    await act(async () => result.current.loadMore());
    expect(apiFetchMock).toHaveBeenCalledTimes(1);
  });

  it("stops paging quietly when a cursor fetch fails, and reload starts fresh", async () => {
    apiFetchMock
      .mockResolvedValueOnce({ items: [post(1)], next_cursor: "c1" })
      .mockRejectedValueOnce(new Error("stale"))
      .mockResolvedValueOnce({ items: [post(9)], next_cursor: null });

    const { result } = renderHook(() => usePagedFeed("7", "tok"));
    await waitFor(() => expect(result.current.posts).toHaveLength(1));

    await act(async () => result.current.loadMore());
    // List unchanged, no error surfaced, paging stopped.
    expect(result.current.posts).toHaveLength(1);
    expect(result.current.error).toBeNull();
    await act(async () => result.current.loadMore());
    expect(apiFetchMock).toHaveBeenCalledTimes(2);

    // reload() re-ranks from scratch.
    await act(async () => result.current.reload());
    await waitFor(() => expect(result.current.posts!.map((p) => Number(p.id))).toEqual([9]));
  });
});
