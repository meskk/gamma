import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { replaceMock, authState } = vi.hoisted(() => ({
  replaceMock: vi.fn(),
  authState: { ready: true, token: null as string | null, isOperator: false },
}));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ replace: replaceMock, push: vi.fn(), back: vi.fn() }),
}));
vi.mock("./auth", () => ({ useAuth: () => ({ ...authState }) }));

import { useRequireOperator } from "./useRequireOperator";

beforeEach(() => {
  replaceMock.mockReset();
  authState.ready = true;
  authState.token = null;
  authState.isOperator = false;
});

describe("useRequireOperator (guards all /admin pages at one seam)", () => {
  it("waits while the session restore is pending", () => {
    authState.ready = false;
    renderHook(() => useRequireOperator());
    expect(replaceMock).not.toHaveBeenCalled();
  });

  it("sends unauthenticated visitors to /login", () => {
    renderHook(() => useRequireOperator());
    expect(replaceMock).toHaveBeenCalledWith("/login");
  });

  it("sends authenticated non-operators to /feed", () => {
    authState.token = "tok";
    renderHook(() => useRequireOperator());
    expect(replaceMock).toHaveBeenCalledWith("/feed");
  });

  it("lets operators through without redirecting", () => {
    authState.token = "tok";
    authState.isOperator = true;
    renderHook(() => useRequireOperator());
    expect(replaceMock).not.toHaveBeenCalled();
  });
});
