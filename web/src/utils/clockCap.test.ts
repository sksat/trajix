import { describe, it, expect } from "vitest";
import { capClockAdvance, MAX_FRAME_WALL_MS } from "./clockCap";

describe("capClockAdvance", () => {
  describe("normal frames (no capping)", () => {
    it("returns original delta at 60fps with 50x multiplier", () => {
      const simDelta = 0.833; // ~50 * 16.7ms
      expect(capClockAdvance(simDelta, 16.7, 50)).toBe(simDelta);
    });

    it("returns original delta at 30fps with 100x multiplier", () => {
      const simDelta = 3.33; // ~100 * 33.3ms
      expect(capClockAdvance(simDelta, 33.3, 100)).toBe(simDelta);
    });

    it("returns original at exactly max wall delta", () => {
      const simDelta = 5; // 50 * 100ms
      expect(capClockAdvance(simDelta, MAX_FRAME_WALL_MS, 50)).toBe(simDelta);
    });
  });

  describe("stall frames (capping applies)", () => {
    it("caps 500ms stall at 50x to 5s sim advance", () => {
      // Without cap: 500ms * 50 = 25s. With cap: 100ms * 50 = 5s
      expect(capClockAdvance(25, 500, 50)).toBeCloseTo(5);
    });

    it("caps 500ms stall at 500x to 50s sim advance", () => {
      expect(capClockAdvance(250, 500, 500)).toBeCloseTo(50);
    });

    it("caps 300ms stall at 100x to 10s sim advance", () => {
      expect(capClockAdvance(30, 300, 100)).toBeCloseTo(10);
    });

    it("caps just above max wall delta", () => {
      // 101ms wall, multiplier 50: should cap to 100ms * 50 = 5s
      expect(capClockAdvance(5.05, 101, 50)).toBeCloseTo(5);
    });
  });

  describe("edge cases", () => {
    it("does not cap negative delta (backwards seek)", () => {
      expect(capClockAdvance(-10, 16.7, 50)).toBe(-10);
    });

    it("does not cap zero delta", () => {
      expect(capClockAdvance(0, 16.7, 50)).toBe(0);
    });

    it("supports custom max wall delta", () => {
      // 200ms cap, multiplier 50: max advance = 200ms * 50 = 10s
      expect(capClockAdvance(25, 500, 50, 200)).toBeCloseTo(10);
    });

    it("handles multiplier of 1 (real-time)", () => {
      // 500ms stall, multiplier 1: cap to 100ms * 1 = 0.1s
      expect(capClockAdvance(0.5, 500, 1)).toBeCloseTo(0.1);
    });
  });
});
