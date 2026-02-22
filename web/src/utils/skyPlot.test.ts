import { describe, it, expect } from "vitest";
import {
  skyPlotProject,
  cn0ToColor,
  constellationLabel,
  deduplicateSatellites,
} from "./skyPlot";
import type { SkyPlotSatellite } from "./skyPlot";

// ────────────────────────────────────────────
// Polar projection
// ────────────────────────────────────────────

describe("skyPlotProject", () => {
  const R = 100; // Radius for tests

  it("zenith (el=90) maps to center", () => {
    const { x, y } = skyPlotProject(0, 90, R);
    expect(x).toBeCloseTo(R, 1);
    expect(y).toBeCloseTo(R, 1);
  });

  it("north horizon (az=0, el=0) maps to top center", () => {
    const { x, y } = skyPlotProject(0, 0, R);
    expect(x).toBeCloseTo(R, 1); // center horizontally
    expect(y).toBeCloseTo(0, 1); // top
  });

  it("east horizon (az=90, el=0) maps to right center", () => {
    const { x, y } = skyPlotProject(90, 0, R);
    expect(x).toBeCloseTo(2 * R, 1); // right edge
    expect(y).toBeCloseTo(R, 1); // center vertically
  });

  it("south horizon (az=180, el=0) maps to bottom center", () => {
    const { x, y } = skyPlotProject(180, 0, R);
    expect(x).toBeCloseTo(R, 1); // center horizontally
    expect(y).toBeCloseTo(2 * R, 1); // bottom
  });

  it("west horizon (az=270, el=0) maps to left center", () => {
    const { x, y } = skyPlotProject(270, 0, R);
    expect(x).toBeCloseTo(0, 1); // left edge
    expect(y).toBeCloseTo(R, 1); // center vertically
  });

  it("45° elevation is halfway between center and edge", () => {
    const { x, y } = skyPlotProject(0, 45, R);
    // North at 45° elevation: x=center, y = center - halfRadius
    expect(x).toBeCloseTo(R, 1);
    expect(y).toBeCloseTo(R / 2, 1); // halfway up from center
  });

  it("different radius scales output proportionally", () => {
    const r50 = skyPlotProject(90, 0, 50);
    const r200 = skyPlotProject(90, 0, 200);
    // East horizon: x should be at 2*radius
    expect(r50.x).toBeCloseTo(100, 1);
    expect(r200.x).toBeCloseTo(400, 1);
  });
});

// ────────────────────────────────────────────
// CN0 color
// ────────────────────────────────────────────

describe("cn0ToColor", () => {
  it("strong signal (>=35) is green", () => {
    expect(cn0ToColor(40)).toBe("#4caf50");
    expect(cn0ToColor(35)).toBe("#4caf50");
  });

  it("medium signal (25-35) is yellow", () => {
    expect(cn0ToColor(30)).toBe("#ffeb3b");
    expect(cn0ToColor(25)).toBe("#ffeb3b");
  });

  it("weak signal (15-25) is orange", () => {
    expect(cn0ToColor(20)).toBe("#ff9800");
    expect(cn0ToColor(15)).toBe("#ff9800");
  });

  it("very weak signal (<15) is red", () => {
    expect(cn0ToColor(10)).toBe("#f44336");
    expect(cn0ToColor(0)).toBe("#f44336");
  });
});

// ────────────────────────────────────────────
// Constellation labels
// ────────────────────────────────────────────

describe("constellationLabel", () => {
  it("GPS is G", () => expect(constellationLabel(1)).toBe("G"));
  it("GLONASS is R", () => expect(constellationLabel(3)).toBe("R"));
  it("Galileo is E", () => expect(constellationLabel(6)).toBe("E"));
  it("BeiDou is C", () => expect(constellationLabel(5)).toBe("C"));
  it("QZSS is J", () => expect(constellationLabel(4)).toBe("J"));
  it("unknown is ?", () => expect(constellationLabel(99)).toBe("?"));
});

// ────────────────────────────────────────────
// Deduplication
// ────────────────────────────────────────────

describe("deduplicateSatellites", () => {
  function makeSat(
    constellation: number,
    svid: number,
    cn0: number,
  ): SkyPlotSatellite {
    return {
      constellation,
      svid,
      azimuth_deg: 180,
      elevation_deg: 45,
      cn0_dbhz: cn0,
      used_in_fix: true,
    };
  }

  it("keeps unique satellites as-is", () => {
    const sats = [makeSat(1, 2, 30), makeSat(1, 3, 25), makeSat(3, 9, 28)];
    const result = deduplicateSatellites(sats);
    expect(result.length).toBe(3);
  });

  it("deduplicates same satellite, keeps highest CN0", () => {
    const sats = [
      makeSat(1, 2, 25), // GPS svid=2, lower CN0
      makeSat(1, 2, 30), // GPS svid=2, higher CN0 (should keep)
      makeSat(3, 9, 28), // Different satellite
    ];
    const result = deduplicateSatellites(sats);
    expect(result.length).toBe(2);
    const gps2 = result.find((s) => s.constellation === 1 && s.svid === 2);
    expect(gps2!.cn0_dbhz).toBe(30); // Highest CN0 kept
  });

  it("handles empty input", () => {
    expect(deduplicateSatellites([])).toEqual([]);
  });

  it("different constellations with same svid are separate", () => {
    const sats = [
      makeSat(1, 5, 30), // GPS svid=5
      makeSat(6, 5, 35), // Galileo svid=5
    ];
    const result = deduplicateSatellites(sats);
    expect(result.length).toBe(2);
  });
});
