import { cleanup, render, screen } from "@testing-library/react";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Controllable route params + a stubbed apiFetch. The signed-in viewer is user 1,
// who follows profile 5 but NOT profile 9.
const { paramsRef, apiFetchMock } = vi.hoisted(() => ({
  paramsRef: { current: { id: "5" } as { id: string } },
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
vi.mock("@/components/PostCard", () => ({ PostCard: () => null }));

import ProfilePage from "./page";

beforeEach(() => {
  paramsRef.current = { id: "5" };
  apiFetchMock.mockReset();
  apiFetchMock.mockImplementation((path: string) => {
    if (/^\/users\/\d+$/.test(path)) {
      const id = path.split("/")[2];
      return Promise.resolve({
        id: Number(id),
        created_at: new Date("2026-01-01").toISOString(),
        declared_categories: [],
        bot_gate_v: false,
        likes_received: 2,
      });
    }
    if (path.startsWith("/posts?")) return Promise.resolve([]);
    // The viewer's own follow list: follows 5, not 9.
    if (path === "/users/1/following") {
      return Promise.resolve([{ follower_id: 1, followee_id: 5 }]);
    }
    if (/^\/users\/\d+\/following$/.test(path)) return Promise.resolve([]); // a profile's count
    return Promise.resolve(undefined);
  });
});
afterEach(() => cleanup());

describe("ProfilePage", () => {
  it("resets follow state across navigation (no stale Unfollow on the next profile)", async () => {
    const { rerender } = render(<ProfilePage />);
    // The Glass redesign renders the handle as a styled <span>, not a heading.
    await screen.findByText("@user-5");
    // Viewer follows 5 → the button reads "Unfollow".
    await screen.findByRole("button", { name: "Entfolgen" });

    // Navigate to profile 9, whom the viewer does NOT follow.
    act(() => {
      paramsRef.current = { id: "9" };
    });
    rerender(<ProfilePage />);
    await screen.findByText("@user-9");

    // The regression: without the reset, profile 5's `following=true` lingered and
    // showed a stale "Unfollow" for profile 9.
    await screen.findByRole("button", { name: "Folgen" });
    expect(screen.queryByRole("button", { name: "Entfolgen" })).toBeNull();
  });
});
