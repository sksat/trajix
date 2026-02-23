import { describe, it, expect } from "vitest";
import {
  lowerBoundNlp,
  findActiveNlpFixes,
  nlpStyle,
  nlpRenderHeight,
  type NlpFixEntry,
} from "./nlpFilter";

// ────────────────────────────────────────────
// Helper
// ────────────────────────────────────────────

function makeFix(
  timeMs: number,
  quality: "GapFallback" | "Rejected" = "GapFallback",
  accuracy: number | null = 400,
): NlpFixEntry {
  return {
    fix: { unix_time_ms: timeMs, accuracy_m: accuracy },
    quality,
  };
}

// ────────────────────────────────────────────
// lowerBoundNlp
// ────────────────────────────────────────────

describe("lowerBoundNlp", () => {
  it("returns 0 for empty array", () => {
    expect(lowerBoundNlp([], 1000)).toBe(0);
  });

  it("returns fixes.length when all before target", () => {
    const fixes = [makeFix(100), makeFix(200), makeFix(300)];
    expect(lowerBoundNlp(fixes, 500)).toBe(3);
  });

  it("returns 0 when all after target", () => {
    const fixes = [makeFix(100), makeFix(200), makeFix(300)];
    expect(lowerBoundNlp(fixes, 50)).toBe(0);
  });

  it("returns index of exact match", () => {
    const fixes = [makeFix(100), makeFix(200), makeFix(300)];
    expect(lowerBoundNlp(fixes, 200)).toBe(1);
  });

  it("returns higher index when between two entries", () => {
    const fixes = [makeFix(100), makeFix(300)];
    expect(lowerBoundNlp(fixes, 200)).toBe(1);
  });

  it("returns first index for duplicate timestamps", () => {
    const fixes = [makeFix(100), makeFix(200), makeFix(200), makeFix(300)];
    expect(lowerBoundNlp(fixes, 200)).toBe(1);
  });

  it("returns 0 for single element equal to target", () => {
    const fixes = [makeFix(100)];
    expect(lowerBoundNlp(fixes, 100)).toBe(0);
  });

  it("returns 1 for single element before target", () => {
    const fixes = [makeFix(100)];
    expect(lowerBoundNlp(fixes, 200)).toBe(1);
  });
});

// ────────────────────────────────────────────
// findActiveNlpFixes
// ────────────────────────────────────────────

describe("findActiveNlpFixes", () => {
  it("returns [] for empty array", () => {
    expect(findActiveNlpFixes([], 1000)).toEqual([]);
  });

  it("returns [] when currentMs before all fixes", () => {
    const fixes = [makeFix(5000), makeFix(6000)];
    expect(findActiveNlpFixes(fixes, 1000)).toEqual([]);
  });

  it("returns single fix before currentMs (lingering)", () => {
    const fix = makeFix(1000);
    expect(findActiveNlpFixes([fix], 2000)).toEqual([fix]);
  });

  it("returns fix at exactly currentMs", () => {
    const fix = makeFix(1000);
    expect(findActiveNlpFixes([fix], 1000)).toEqual([fix]);
  });

  it("returns [] when fix too old (> maxLingerMs)", () => {
    const fix = makeFix(1000);
    // default maxLingerMs = 5000, so fix at 1000 with currentMs at 7000 is too old
    expect(findActiveNlpFixes([fix], 6001)).toEqual([]);
  });

  it("returns fix at exactly maxLingerMs boundary", () => {
    const fix = makeFix(1000);
    // currentMs - fix.time = 5000 exactly = maxLingerMs (default)
    expect(findActiveNlpFixes([fix], 6000)).toEqual([fix]);
  });

  it("excludes fix at maxLingerMs + 1", () => {
    const fix = makeFix(1000);
    expect(findActiveNlpFixes([fix], 6001)).toEqual([]);
  });

  it("respects custom maxLingerMs", () => {
    const fix = makeFix(1000);
    // custom maxLingerMs = 2000
    expect(findActiveNlpFixes([fix], 3000, 2000)).toEqual([fix]);
    expect(findActiveNlpFixes([fix], 3001, 2000)).toEqual([]);
  });

  it("returns burst: 3 fixes within 1s before currentMs", () => {
    const fixes = [makeFix(1000), makeFix(1300), makeFix(1600)];
    // currentMs=2000, all within 1s of each other and within maxLingerMs
    const result = findActiveNlpFixes(fixes, 2000);
    expect(result).toEqual(fixes);
  });

  it("burst cutoff: excludes fix beyond 1s gap from most recent", () => {
    // Fixes at 1000 (gap > 1s to 2500), 2500, 2800 (burst pair)
    const fixes = [makeFix(1000), makeFix(2500), makeFix(2800)];
    // currentMs=3000, most recent is 2800, then 2500 is within 1s of 2800
    // but 1000 is 1500ms away from 2500 -> excluded from burst
    const result = findActiveNlpFixes(fixes, 3000);
    expect(result).toEqual([makeFix(2500), makeFix(2800)]);
  });

  it("excludes future fixes (after currentMs)", () => {
    const fixes = [makeFix(1000), makeFix(5000)];
    const result = findActiveNlpFixes(fixes, 2000);
    expect(result).toEqual([makeFix(1000)]);
  });

  it("returns most recent fix, not older ones beyond burst gap", () => {
    // fix at t=100, gap, fix at t=5000
    const fixes = [makeFix(100), makeFix(5000)];
    const result = findActiveNlpFixes(fixes, 5500);
    // fix at 100 is too far from fix at 5000 (>1s gap) -> only the most recent
    expect(result).toEqual([makeFix(5000)]);
  });

  it("handles many fixes, returns only recent burst", () => {
    const fixes = [
      makeFix(1000),
      makeFix(2000),
      makeFix(3000),
      makeFix(10000),
      makeFix(10200),
      makeFix(10400),
    ];
    // At time 11000, recent burst is 10000-10400
    const result = findActiveNlpFixes(fixes, 11000);
    expect(result).toEqual([makeFix(10000), makeFix(10200), makeFix(10400)]);
  });
});

// ────────────────────────────────────────────
// nlpStyle
// ────────────────────────────────────────────

// ────────────────────────────────────────────
// nlpRenderHeight
// ────────────────────────────────────────────

describe("nlpRenderHeight", () => {
  it("returns 0 for null altitude (most NLP fixes)", () => {
    expect(nlpRenderHeight(null)).toBe(0);
  });

  it("returns 0 even when NLP fix has altitude", () => {
    // NLP markers should always be ground-clamped, never at GPS altitude
    expect(nlpRenderHeight(288)).toBe(0);
  });

  it("returns 0 for high altitude (flight scenario)", () => {
    // Even if NLP reports altitude during flight, render at ground
    expect(nlpRenderHeight(10000)).toBe(0);
  });

  it("returns 0 for zero altitude", () => {
    expect(nlpRenderHeight(0)).toBe(0);
  });
});

// ────────────────────────────────────────────
// nlpStyle
// ────────────────────────────────────────────

describe("nlpStyle", () => {
  it("GapFallback returns orange style", () => {
    const style = nlpStyle("GapFallback");
    expect(style.hue).toBe("orange");
    expect(style.pointSize).toBe(8);
    expect(style.pointAlpha).toBeCloseTo(0.9);
    expect(style.circleAlpha).toBeCloseTo(0.35);
  });

  it("Rejected returns red style", () => {
    const style = nlpStyle("Rejected");
    expect(style.hue).toBe("red");
    expect(style.pointSize).toBe(6);
    expect(style.pointAlpha).toBeCloseTo(0.7);
    expect(style.circleAlpha).toBeCloseTo(0.25);
  });
});
