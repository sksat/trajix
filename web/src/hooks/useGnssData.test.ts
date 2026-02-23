// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useGnssData } from "./useGnssData";

describe("useGnssData", () => {
  it("initial state is idle", () => {
    const { result } = renderHook(() => useGnssData());
    expect(result.current.state.status).toBe("idle");
  });

  it("reset() returns state to idle", () => {
    const { result } = renderHook(() => useGnssData());
    // Even from idle, reset should keep idle
    act(() => result.current.reset());
    expect(result.current.state.status).toBe("idle");
  });

  it("exposes reset function", () => {
    const { result } = renderHook(() => useGnssData());
    expect(typeof result.current.reset).toBe("function");
  });
});
