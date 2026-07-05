import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { apiFetchMock } = vi.hoisted(() => ({ apiFetchMock: vi.fn() }));
vi.mock("@/lib/api", () => ({ apiFetch: apiFetchMock }));
vi.mock("@/lib/useRequireOperator", () => ({
  useRequireOperator: () => ({ token: "tok", ready: true, isOperator: true }),
}));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), back: vi.fn() }),
}));

import ReportsPage from "./page";

const reported = (postId: number, hidden: boolean) => ({
  post_id: postId,
  report_count: 3,
  hidden,
});

beforeEach(() => {
  // Block body ON PURPOSE: mockReset() returns the mock, and a function
  // returned from a vitest hook is invoked as a TEARDOWN callback — which
  // would call the mock bare and leak an unhandled rejection.
  apiFetchMock.mockReset();
});
afterEach(() => cleanup());

describe("admin reports page", () => {
  it("loads the queue, takes a post down, and reloads the list", async () => {
    // Default-resolve (never default-reject): vitest flags any stray rejected
    // promise from mock plumbing as an unhandled rejection.
    apiFetchMock.mockImplementation((path?: string) => {
      if (path === "/reports") return Promise.resolve([reported(7, false)]);
      return Promise.resolve(undefined);
    });

    render(<ReportsPage />);
    await screen.findByText("post 7");

    fireEvent.click(screen.getByRole("button", { name: "Take down" }));
    // The action POSTs, then reload() refetches the queue.
    await waitFor(() => {
      const calls = apiFetchMock.mock.calls.map((c) => c[0]);
      expect(calls.filter((p) => p === "/reports").length).toBe(2);
      expect(calls).toContain("/posts/7/takedown");
    });
  });

  it("maps a load failure to the queue error message", async () => {
    // Reject ONLY the queue fetch; everything else resolves (see above).
    apiFetchMock.mockImplementation((path?: string) =>
      path === "/reports" ? Promise.reject(new Error("nope")) : Promise.resolve(undefined),
    );
    render(<ReportsPage />);
    await screen.findByText("Could not load the moderation queue.");
  });
});
