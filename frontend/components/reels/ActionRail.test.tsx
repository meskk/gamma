import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Post } from "@contract/Post";

// Mutable features mock so each test can flip the launch flags (P-1).
const { featuresMock } = vi.hoisted(() => ({
  featuresMock: { tips: false, saves: false, gemUnlock: false },
}));
vi.mock("@/lib/features", () => ({ FEATURES: featuresMock }));
vi.mock("@/lib/api", () => ({ apiFetch: vi.fn() }));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), back: vi.fn() }),
}));

import { ActionRail } from "./ActionRail";

const post = { id: 1n, author_id: 2n, category: null, body: "hi" } as unknown as Post;

beforeEach(() => {
  featuresMock.tips = false;
  featuresMock.saves = false;
});
afterEach(() => cleanup());

describe("ActionRail launch flags (P-1)", () => {
  it("hides the tip and save buttons by default", () => {
    render(<ActionRail post={post} token="tok" />);

    // Core actions stay.
    expect(screen.getByRole("button", { name: "Gefällt mir" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Kommentare öffnen" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Teilen" })).toBeTruthy();

    // Hidden: the dead tip button and the local-only save toggle.
    expect(screen.queryByRole("button", { name: /Trinkgeld/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Speichern" })).toBeNull();
  });

  it("brings them back when the flags are on (nothing was deleted)", () => {
    featuresMock.tips = true;
    featuresMock.saves = true;
    render(<ActionRail post={post} token="tok" />);

    expect(screen.getByRole("button", { name: /Trinkgeld/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Speichern" })).toBeTruthy();
  });
});
