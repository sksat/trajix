import { describe, it, expect } from "vitest";
import { filterAltitudeSpikes, smoothAltitudes } from "./altitude";
import type { FixRecord } from "../types/gnss";

// Helper to create minimal FixRecord for altitude tests
function makeFix(unix_time_ms: number, altitude_m: number | null = 100): FixRecord {
  return {
    unix_time_ms,
    provider: "Gps",
    latitude_deg: 36.0,
    longitude_deg: 140.0,
    altitude_m,
    speed_mps: null,
    bearing_deg: null,
    accuracy_m: 5,
    vertical_accuracy_m: 3,
    speed_accuracy_mps: null,
    vertical_speed_accuracy_mps: null,
    bearing_accuracy_deg: null,
  } as FixRecord;
}

// Helper: create fixes at 1-second intervals with given altitudes
function makeTrack(altitudes: number[], startMs = 0): { fixes: FixRecord[]; heights: number[] } {
  const fixes = altitudes.map((alt, i) => makeFix(startMs + i * 1000, alt));
  return { fixes, heights: [...altitudes] };
}

// ────────────────────────────────────────────
// filterAltitudeSpikes — median-based spike filter
// ────────────────────────────────────────────

describe("filterAltitudeSpikes", () => {
  it("returns original heights when no spikes present", () => {
    const { fixes, heights } = makeTrack([100, 101, 102, 103, 104]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights);
    expect(filteredHeights).toEqual(heights);
    expect(stats.pointsReplaced).toBe(0);
  });

  it("returns empty array for empty input", () => {
    const { filteredHeights, stats } = filterAltitudeSpikes([], []);
    expect(filteredHeights).toEqual([]);
    expect(stats.pointsReplaced).toBe(0);
  });

  it("returns single point unchanged", () => {
    const { fixes, heights } = makeTrack([100]);
    const { filteredHeights } = filterAltitudeSpikes(fixes, heights);
    expect(filteredHeights).toEqual([100]);
  });

  it("removes single-point spike (high)", () => {
    // Normal track at ~100m with one point spiking to 200m
    const { fixes, heights } = makeTrack([100, 100, 100, 200, 100, 100, 100]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // Point at index 3 should be replaced (median of neighbors ≈ 100)
    expect(filteredHeights[3]).toBeCloseTo(100, 0);
    expect(stats.pointsReplaced).toBe(1);
    // Other points unchanged
    expect(filteredHeights[0]).toBe(100);
    expect(filteredHeights[6]).toBe(100);
  });

  it("removes single-point spike (low)", () => {
    const { fixes, heights } = makeTrack([100, 100, 100, 0, 100, 100, 100]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(filteredHeights[3]).toBeCloseTo(100, 0);
    expect(stats.pointsReplaced).toBe(1);
  });

  it("removes multi-point spike plateau", () => {
    const { fixes, heights } = makeTrack([100, 100, 100, 200, 210, 200, 100, 100, 100]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // Points 3, 4, 5 should be replaced
    expect(filteredHeights[3]).toBeLessThan(150);
    expect(filteredHeights[4]).toBeLessThan(150);
    expect(filteredHeights[5]).toBeLessThan(150);
    expect(stats.pointsReplaced).toBeGreaterThanOrEqual(2);
  });

  it("preserves gradual climb", () => {
    // 10m per second for 10 seconds = 100m total climb
    const alts = Array.from({ length: 11 }, (_, i) => 100 + i * 10);
    const { fixes, heights } = makeTrack(alts);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // All points should be unchanged (gradual climb is not a spike)
    for (let i = 0; i < alts.length; i++) {
      expect(filteredHeights[i]).toBeCloseTo(alts[i]!, 1);
    }
    expect(stats.pointsReplaced).toBe(0);
  });

  it("handles multiple separate spikes", () => {
    // Two spikes in an otherwise flat track
    const alts = [100, 100, 100, 200, 100, 100, 100, 100, 100, 300, 100, 100, 100];
    const { fixes, heights } = makeTrack(alts);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(filteredHeights[3]).toBeCloseTo(100, 0);
    expect(filteredHeights[9]).toBeCloseTo(100, 0);
    expect(stats.pointsReplaced).toBe(2);
  });

  it("does not modify input arrays", () => {
    const { fixes, heights } = makeTrack([100, 100, 200, 100, 100]);
    const original = [...heights];
    filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(heights).toEqual(original);
  });

  it("handles GPS/FLP interleaving pattern (small jitter)", () => {
    // Alternating 800/803m — this is normal GPS/FLP interleaving, NOT a spike
    const alts = [800, 803, 800, 803, 800, 803, 800, 803, 800, 803, 800];
    const { fixes, heights } = makeTrack(alts);
    const { stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // 3m jitter should NOT be filtered (below 30m threshold)
    expect(stats.pointsReplaced).toBe(0);
  });

  it("handles GPS/FLP interleaving with occasional spike", () => {
    // Normal ~2m jitter with one 80m spike
    const alts = [800, 802, 800, 802, 800, 880, 800, 802, 800, 802, 800];
    const { fixes, heights } = makeTrack(alts);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // Only the 880m spike should be filtered
    expect(filteredHeights[5]).toBeLessThan(830);
    expect(stats.pointsReplaced).toBe(1);
  });

  it("uses custom window size", () => {
    // With a narrow window (±1 = 3-point window), a small deviation might pass
    const alts = [100, 100, 140, 100, 100];
    const { fixes, heights } = makeTrack(alts);
    const r1 = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30, halfWindow: 2 });
    expect(r1.filteredHeights[2]).toBeCloseTo(100, 0); // filtered
    expect(r1.stats.pointsReplaced).toBe(1);
  });

  it("handles spike at start of array", () => {
    const { fixes, heights } = makeTrack([300, 100, 100, 100, 100, 100, 100]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    // First point should be replaced (median of available neighbors ≈ 100)
    expect(filteredHeights[0]).toBeCloseTo(100, 0);
    expect(stats.pointsReplaced).toBe(1);
  });

  it("handles spike at end of array", () => {
    const { fixes, heights } = makeTrack([100, 100, 100, 100, 100, 100, 300]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(filteredHeights[6]).toBeCloseTo(100, 0);
    expect(stats.pointsReplaced).toBe(1);
  });

  it("returns meaningful statistics", () => {
    const alts = [100, 100, 100, 250, 100, 100, 100, 100, 300, 100, 100];
    const { fixes, heights } = makeTrack(alts);
    const { stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(stats.pointsReplaced).toBe(2);
    expect(stats.maxDeviation).toBeGreaterThanOrEqual(150); // 300 - 100 = 200m deviation
  });

  it("handles all-same altitude", () => {
    const { fixes, heights } = makeTrack([500, 500, 500, 500, 500]);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights);
    expect(filteredHeights).toEqual([500, 500, 500, 500, 500]);
    expect(stats.pointsReplaced).toBe(0);
  });

  it("handles two-point input", () => {
    const { fixes, heights } = makeTrack([100, 200]);
    const { filteredHeights } = filterAltitudeSpikes(fixes, heights);
    // Can't meaningfully filter 2 points — return as-is
    expect(filteredHeights).toEqual([100, 200]);
  });

  it("realistic mountain hiking: altitude 200→900m over 2h", () => {
    // Simulate gradual climb with occasional 50m GPS spike
    const n = 100;
    const alts: number[] = [];
    for (let i = 0; i < n; i++) {
      const base = 200 + (700 * i) / n;
      // Add spike at index 30 and 70
      if (i === 30 || i === 70) {
        alts.push(base + 150); // 150m spike
      } else {
        alts.push(base);
      }
    }
    const { fixes, heights } = makeTrack(alts);
    const { filteredHeights, stats } = filterAltitudeSpikes(fixes, heights, { deviationThreshold: 30 });
    expect(stats.pointsReplaced).toBe(2);
    // Spike points should be close to their neighbors
    const expected30 = 200 + (700 * 30) / n;
    const expected70 = 200 + (700 * 70) / n;
    // Median includes slope, so allow ±15m tolerance
    expect(Math.abs(filteredHeights[30]! - expected30)).toBeLessThan(15);
    expect(Math.abs(filteredHeights[70]! - expected70)).toBeLessThan(15);
    // Non-spike points should be unchanged
    expect(filteredHeights[0]).toBeCloseTo(200, 0);
    expect(filteredHeights[50]).toBeCloseTo(200 + 350, 0);
  });
});

// ────────────────────────────────────────────
// smoothAltitudes — time-aware moving average
// ────────────────────────────────────────────

describe("smoothAltitudes", () => {
  it("returns empty array for empty input", () => {
    expect(smoothAltitudes([], [])).toEqual([]);
  });

  it("returns single point unchanged", () => {
    const { fixes, heights } = makeTrack([100]);
    expect(smoothAltitudes(fixes, heights)).toEqual([100]);
  });

  it("returns two points unchanged", () => {
    const { fixes, heights } = makeTrack([100, 200]);
    expect(smoothAltitudes(fixes, heights)).toEqual([100, 200]);
  });

  it("smooths constant altitude to same value", () => {
    const { fixes, heights } = makeTrack([500, 500, 500, 500, 500]);
    const result = smoothAltitudes(fixes, heights);
    expect(result).toEqual([500, 500, 500, 500, 500]);
  });

  it("reduces jitter in flat track", () => {
    // Alternating 100/110m — smoothing should bring values closer together
    const alts = [100, 110, 100, 110, 100, 110, 100, 110, 100, 110, 100];
    const { fixes, heights } = makeTrack(alts);
    const result = smoothAltitudes(fixes, heights, { halfWindow: 3 });

    // Center points should be closer to 105 (mean of 100 and 110)
    for (let i = 3; i < result.length - 3; i++) {
      expect(Math.abs(result[i]! - 105)).toBeLessThan(3);
    }
  });

  it("preserves gradual climb trend", () => {
    // Linear climb: smoothing a linear function should return the same function
    const alts = Array.from({ length: 11 }, (_, i) => 100 + i * 10);
    const { fixes, heights } = makeTrack(alts);
    const result = smoothAltitudes(fixes, heights, { halfWindow: 3 });

    // Center points should closely match original (linear avg of linear = linear)
    for (let i = 3; i < result.length - 3; i++) {
      expect(result[i]).toBeCloseTo(alts[i]!, 1);
    }
  });

  it("does not average across time gaps", () => {
    // Two clusters separated by a 10-second gap (> 3s default threshold)
    const fixes = [
      makeFix(0, 100), makeFix(1000, 100), makeFix(2000, 100),
      // 10-second gap
      makeFix(12000, 200), makeFix(13000, 200), makeFix(14000, 200),
    ];
    const heights = [100, 100, 100, 200, 200, 200];

    const result = smoothAltitudes(fixes, heights, { halfWindow: 3, gapThresholdMs: 3000 });

    // Points in each cluster should stay near their own altitude
    expect(result[0]).toBeCloseTo(100, 0);
    expect(result[2]).toBeCloseTo(100, 0);
    expect(result[3]).toBeCloseTo(200, 0);
    expect(result[5]).toBeCloseTo(200, 0);
  });

  it("smooths GPS/FLP interleaving jitter", () => {
    // Real-world pattern: GPS=800m, FLP=806m alternating at 1Hz
    const alts = [800, 806, 800, 806, 800, 806, 800, 806, 800, 806, 800];
    const { fixes, heights } = makeTrack(alts);
    const result = smoothAltitudes(fixes, heights, { halfWindow: 3 });

    // Should converge toward ~803m in the middle
    for (let i = 3; i < result.length - 3; i++) {
      expect(Math.abs(result[i]! - 803)).toBeLessThan(2);
    }
  });

  it("does not modify input arrays", () => {
    const { fixes, heights } = makeTrack([100, 110, 100, 110, 100]);
    const original = [...heights];
    smoothAltitudes(fixes, heights);
    expect(heights).toEqual(original);
  });

  it("uses custom window size", () => {
    // Larger window = smoother result
    const alts = [100, 120, 100, 120, 100, 120, 100, 120, 100, 120, 100];
    const { fixes, heights } = makeTrack(alts);

    const small = smoothAltitudes(fixes, heights, { halfWindow: 1 });
    const large = smoothAltitudes(fixes, heights, { halfWindow: 5 });

    // Larger window should produce values closer to the mean (110)
    const smallDeviation = Math.abs(small[5]! - 110);
    const largeDeviation = Math.abs(large[5]! - 110);
    expect(largeDeviation).toBeLessThanOrEqual(smallDeviation);
  });

  it("realistic: smooths noisy mountain hike altitude", () => {
    // Base climb 200→900m with ±5m random-like noise
    const n = 50;
    const noise = [3, -4, 5, -2, 4, -5, 3, -3, 5, -4, 2, -5, 4, -3, 5, -2, 3, -4, 5, -3,
                   4, -5, 2, -4, 5, -3, 3, -5, 4, -2, 5, -4, 3, -5, 2, -3, 4, -5, 5, -2,
                   3, -4, 5, -3, 4, -5, 2, -4, 5, -3];
    const alts = Array.from({ length: n }, (_, i) => 200 + (700 * i) / n + noise[i]!);
    const { fixes, heights } = makeTrack(alts);
    const result = smoothAltitudes(fixes, heights, { halfWindow: 3 });

    // Smoothed values should be closer to the true trend than raw
    let rawError = 0;
    let smoothError = 0;
    for (let i = 3; i < n - 3; i++) {
      const trueAlt = 200 + (700 * i) / n;
      rawError += Math.abs(heights[i]! - trueAlt);
      smoothError += Math.abs(result[i]! - trueAlt);
    }
    expect(smoothError).toBeLessThan(rawError);
  });
});
