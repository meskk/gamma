import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Controllable route params + a stubbed apiFetch, shared into the hoisted mocks.
const { paramsRef, apiFetchMock } = vi.hoisted(() => ({
  paramsRef: { current: { id: "1" } as { id: string } },
  apiFetchMock: vi.fn(),
}));

vi.mock("next/navigation", () => ({
  useParams: () => paramsRef.current,
  useRouter: () => ({ replace: vi.fn(), push: vi.fn() }),
}));
vi.mock("@/lib/useRequireAuth", () => ({
  useRequireAuth: () => ({ token: "t", userId: "1", ready: true, role: "user", isOperator: false }),
}));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  apiFetch: apiFetchMock,
}));
// The post page also renders these; they fetch on their own, so stub them out.
vi.mock("@/components/Comments", () => ({ Comments: () => null }));
vi.mock("@/components/MediaView", () => ({ MediaView: () => null }));

import PostDetailPage from "./page";

function postResponse(id: string) {
  return {
    id: Number(id),
    author_id: 9,
    category: null,
    body: `post ${id}`,
    created_at: new Date("2026-01-01").toISOString(),
    popularity_score: 0,
    media_id: null,
  };
}

beforeEach(() => {
  paramsRef.current = { id: "1" };
  apiFetchMock.mockReset();
  apiFetchMock.mockImplementation((path: string) => {
    if (path.startsWith("/posts/")) return Promise.resolve(postResponse(path.split("/")[2]));
    return Promise.resolve(undefined); // POST /interactions
  });
});
afterEach(() => cleanup());

describe("PostDetailPage", () => {
  it("resets liked state across navigation, so the next post can still be liked", async () => {
    const { rerender } = render(<PostDetailPage />);
    await screen.findByText("post 1");

    // Like post 1 → the button flips to the liked (disabled) state.
    fireEvent.click(screen.getByRole("button", { name: /Like/ }));
    await vi.waitFor(() => {
      expect(screen.getByRole("button", { name: /Like/ }).textContent).toBe("♥ Liked");
    });

    // Navigate to post 2 (App Router reuses the mounted component across [id]).
    act(() => {
      paramsRef.current = { id: "2" };
    });
    rerender(<PostDetailPage />);
    await screen.findByText("post 2");

    // The regression: without the state reset, `liked` stayed true and the button
    // was stuck on "♥ Liked" + disabled, so post 2 could never be liked.
    const likeButton = screen.getByRole("button", { name: /Like/ }) as HTMLButtonElement;
    expect(likeButton.textContent).toBe("♡ Like");
    expect(likeButton.disabled).toBe(false);
  });
});
