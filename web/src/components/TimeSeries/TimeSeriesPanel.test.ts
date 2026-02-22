import { describe, it, expect } from "vitest";
import { computeChartWidth } from "./TimeSeriesPanel";

describe("computeChartWidth", () => {
  it("mobile (<768px): single column, full width", () => {
    // 720px mobile drawer → 1 col → 720/1 - 8 = 712
    expect(computeChartWidth(720)).toBe(712);
  });

  it("narrow mobile (400px): single column", () => {
    expect(computeChartWidth(400)).toBe(392);
  });

  it("tablet (768px): 2 columns", () => {
    // 768px → 2 cols → 768/2 - 8 = 376
    expect(computeChartWidth(768)).toBe(376);
  });

  it("medium (900px): 2 columns", () => {
    expect(computeChartWidth(900)).toBe(442);
  });

  it("desktop (1000px): 4 columns", () => {
    // 1000px → 4 cols → 1000/4 - 8 = 242
    expect(computeChartWidth(1000)).toBe(242);
  });

  it("wide desktop (1600px): 4 columns", () => {
    expect(computeChartWidth(1600)).toBe(392);
  });

  it("minimum width floor is 200px", () => {
    // Very narrow: 100px → 1 col → 100/1 - 8 = 92 → clamped to 200
    expect(computeChartWidth(100)).toBe(200);
  });

  it("breakpoint boundary: 767px is single column", () => {
    const w767 = computeChartWidth(767);
    const w768 = computeChartWidth(768);
    // 767 → 1 col (full width), 768 → 2 cols (half width)
    expect(w767).toBeGreaterThan(w768);
  });
});
