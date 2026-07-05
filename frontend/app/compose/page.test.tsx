import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Mutable features mock so tests can flip the gem-unlock launch flag (P-1).
const { featuresMock, uploadMock } = vi.hoisted(() => ({
  featuresMock: { tips: false, saves: false, gemUnlock: false },
  uploadMock: vi.fn(),
}));
vi.mock("@/lib/features", () => ({ FEATURES: featuresMock }));
vi.mock("@/lib/api", () => ({ apiFetch: vi.fn() }));
vi.mock("@/lib/mediaUpload", () => ({ uploadMedia: uploadMock }));
vi.mock("@/lib/useRequireAuth", () => ({
  useRequireAuth: () => ({ token: "tok", ready: true }),
}));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn(), replace: vi.fn(), back: vi.fn() }),
}));

import ComposePage from "./page";

function attachFile() {
  const file = new File(["x"], "pic.png", { type: "image/png" });
  const input = document.querySelector('input[type="file"]') as HTMLInputElement;
  fireEvent.change(input, { target: { files: [file] } });
}

beforeEach(() => {
  featuresMock.gemUnlock = false;
  uploadMock.mockReset();
});
afterEach(() => cleanup());

describe("ComposePage gem-unlock flag (P-1)", () => {
  it("hides the unlock-price field by default, even with a file attached", () => {
    render(<ComposePage />);
    attachFile();
    expect(screen.queryByLabelText(/Preis zum Freischalten/)).toBeNull();
  });

  it("shows the price field again when the flag is on", () => {
    featuresMock.gemUnlock = true;
    render(<ComposePage />);
    attachFile();
    expect(screen.getByLabelText(/Preis zum Freischalten/)).toBeTruthy();
  });
});
