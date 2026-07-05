import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Post } from "@contract/Post";

const { loadMoreMock, apiFetchMock } = vi.hoisted(() => ({
  loadMoreMock: vi.fn(),
  apiFetchMock: vi.fn(),
}));

const posts: Post[] = Array.from(
  { length: 12 },
  (_, i) => ({ id: i + 1, author_id: 99, category: null, body: `post ${i + 1}`, media_id: null, created_at: new Date().toISOString() }) as unknown as Post,
);

vi.mock("@/lib/usePagedFeed", () => ({
  usePagedFeed: () => ({ posts, error: null, loadMore: loadMoreMock, reload: vi.fn() }),
}));
vi.mock("@/lib/api", () => ({ apiFetch: apiFetchMock }));
vi.mock("@/lib/features", () => ({ FEATURES: { tips: false, saves: false, gemUnlock: false } }));
vi.mock("./ReelMedia", () => ({ ReelMedia: () => <div data-testid="media" /> }));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), back: vi.fn(), refresh: vi.fn() }),
}));

import { ReelsFeed } from "./ReelsFeed";

beforeEach(() => {
  loadMoreMock.mockReset();
  apiFetchMock.mockReset().mockResolvedValue([]); // the follows fetch
});
afterEach(() => cleanup());

describe("ReelsFeed paging (D2)", () => {
  it("renders the feed and does NOT prefetch while far from the end", async () => {
    render(<ReelsFeed token="tok" userId="7" />);
    await screen.findAllByTestId("media");
    // The pinned caption shows the active reel's author handle.
    expect(screen.getByText("@user-99")).toBeTruthy();
    expect(loadMoreMock).not.toHaveBeenCalled();
  });

  it("pulls the next page when the viewer nears the end of the track", async () => {
    render(<ReelsFeed token="tok" userId="7" />);
    await screen.findAllByTestId("media");

    // Chevron paging is undebounced: step to index 9 (12 posts → threshold
    // fires at idx >= 9).
    const next = screen.getByRole("button", { name: "Nächstes" });
    for (let i = 0; i < 9; i++) fireEvent.click(next);

    await waitFor(() => expect(loadMoreMock).toHaveBeenCalled());
  });
});
