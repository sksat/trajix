import { describe, it, expect } from "vitest";
import {
  geodeticBearing,
  angleDiff,
  createBearingTracker,
  updateBearing,
  resetBearing,
  autoHeading,
  detectUserDrag,
  estimateFrameSpeed,
  computeNarrowFov,
  computeLagDistance,
  computeVisibilityRange,
  computeTargetRange,
  lerpRange,
  adjustPitchForTerrain,
  checkLineOfSight,
  approxCameraPosition,
  occlusionNudgeDirection,
} from "./cameraFollow";

const toRad = (deg: number) => (deg * Math.PI) / 180;

// ────────────────────────────────────────────
// geodeticBearing
// ────────────────────────────────────────────

describe("geodeticBearing", () => {
  it("returns 0 for due-north movement", () => {
    const b = geodeticBearing(toRad(139), toRad(35), toRad(139), toRad(36));
    expect(b).toBeCloseTo(0, 3);
  });

  it("returns π/2 for due-east movement", () => {
    const b = geodeticBearing(toRad(139), toRad(35), toRad(140), toRad(35));
    expect(b).toBeCloseTo(Math.PI / 2, 1);
  });

  it("returns -π/2 for due-west movement", () => {
    const b = geodeticBearing(toRad(140), toRad(35), toRad(139), toRad(35));
    expect(b).toBeCloseTo(-Math.PI / 2, 1);
  });

  it("returns ±π for due-south movement", () => {
    const b = geodeticBearing(toRad(139), toRad(36), toRad(139), toRad(35));
    expect(Math.abs(b)).toBeCloseTo(Math.PI, 3);
  });

  it("returns ~π/4 for northeast movement", () => {
    // Small displacement northeast from equator
    const b = geodeticBearing(0, 0, toRad(0.01), toRad(0.01));
    expect(b).toBeCloseTo(Math.PI / 4, 1);
  });

  it("handles antimeridian crossing eastward (179°E → -179°W)", () => {
    const b = geodeticBearing(toRad(179), toRad(35), toRad(-179), toRad(35));
    // Moving east across antimeridian → bearing ≈ π/2
    expect(b).toBeCloseTo(Math.PI / 2, 0);
  });

  it("handles antimeridian crossing westward (-179°W → 179°E)", () => {
    const b = geodeticBearing(toRad(-179), toRad(35), toRad(179), toRad(35));
    // Moving west across antimeridian → bearing ≈ -π/2
    expect(b).toBeCloseTo(-Math.PI / 2, 0);
  });
});

// ────────────────────────────────────────────
// angleDiff
// ────────────────────────────────────────────

describe("angleDiff", () => {
  it("returns 0 for equal angles", () => {
    expect(angleDiff(1.0, 1.0)).toBeCloseTo(0);
  });

  it("returns positive for counterclockwise difference", () => {
    expect(angleDiff(0.5, 0.0)).toBeCloseTo(0.5);
  });

  it("returns negative for clockwise difference", () => {
    expect(angleDiff(0.0, 0.5)).toBeCloseTo(-0.5);
  });

  it("wraps across ±π boundary", () => {
    // 3.0 - (-3.0) = 6.0 rad, but shortest is 6.0 - 2π ≈ -0.283
    const d = angleDiff(3.0, -3.0);
    expect(d).toBeCloseTo(6.0 - 2 * Math.PI, 5);
  });

  it("handles 2π difference as zero", () => {
    expect(angleDiff(2 * Math.PI, 0)).toBeCloseTo(0, 5);
  });
});

// ────────────────────────────────────────────
// BearingTracker — adaptive EMA
// ────────────────────────────────────────────

describe("BearingTracker", () => {
  it("starts with no bearing", () => {
    const s = createBearingTracker();
    expect(s.hasBearing).toBe(false);
    expect(s.prevLon).toBeNull();
  });

  it("first sample does not produce bearing", () => {
    const s = createBearingTracker();
    const updated = updateBearing(s, toRad(139), toRad(35));
    expect(updated).toBe(false);
    expect(s.hasBearing).toBe(false);
    expect(s.prevLon).not.toBeNull();
  });

  it("second sample produces initial bearing", () => {
    const s = createBearingTracker();
    updateBearing(s, toRad(139), toRad(35), { sampleRadians: 0 });
    const updated = updateBearing(s, toRad(139), toRad(35.01), { sampleRadians: 0 });
    expect(updated).toBe(true);
    expect(s.hasBearing).toBe(true);
    // Moving north → bearing ≈ 0
    expect(s.bearing).toBeCloseTo(0, 1);
  });

  it("tracks straight-line bearing correctly", () => {
    const s = createBearingTracker();
    const opts = { sampleRadians: 0 };
    // Move due east in small steps
    for (let i = 0; i <= 10; i++) {
      updateBearing(s, toRad(139 + i * 0.001), toRad(35), opts);
    }
    expect(s.hasBearing).toBe(true);
    // Bearing should be ~π/2 (east)
    expect(s.bearing).toBeCloseTo(Math.PI / 2, 1);
  });

  it("uses small alpha on straight path", () => {
    const s = createBearingTracker();
    const opts = { sampleRadians: 0, alphaMin: 0.05, alphaMax: 0.4 };
    // All samples moving north — delta ≈ 0 → alpha ≈ alphaMin
    for (let i = 0; i <= 20; i++) {
      updateBearing(s, toRad(139), toRad(35 + i * 0.001), opts);
    }
    // Bearing should be very close to 0 (north) — converged with small alpha
    expect(s.bearing).toBeCloseTo(0, 1);
  });

  it("responds quickly to sharp turn", () => {
    const s = createBearingTracker();
    const opts = { sampleRadians: 0, alphaMin: 0.05, alphaMax: 0.4, turnThreshold: Math.PI / 4 };

    // 10 samples moving north
    for (let i = 0; i <= 10; i++) {
      updateBearing(s, toRad(139), toRad(35 + i * 0.001), opts);
    }
    expect(s.bearing).toBeCloseTo(0, 1); // north

    // Now turn east — sharp 90° change should use high alpha
    const lat = toRad(35 + 10 * 0.001);
    for (let i = 1; i <= 5; i++) {
      updateBearing(s, toRad(139 + i * 0.001), lat, opts);
    }
    // After 5 samples with high alpha, bearing should be well past 45°
    expect(s.bearing).toBeGreaterThan(Math.PI / 4);
  });

  it("ignores sub-threshold movement", () => {
    const s = createBearingTracker();
    const opts = { sampleRadians: 0.001 };
    updateBearing(s, toRad(139), toRad(35), opts);
    // Tiny movement below threshold
    const updated = updateBearing(s, toRad(139) + 0.0001, toRad(35), opts);
    expect(updated).toBe(false);
    expect(s.hasBearing).toBe(false);
  });

  it("reset clears all state", () => {
    const s = createBearingTracker();
    updateBearing(s, toRad(139), toRad(35), { sampleRadians: 0 });
    updateBearing(s, toRad(139.01), toRad(35), { sampleRadians: 0 });
    expect(s.hasBearing).toBe(true);

    resetBearing(s);
    expect(s.hasBearing).toBe(false);
    expect(s.prevLon).toBeNull();
    expect(s.bearing).toBe(0);
  });
});

// ────────────────────────────────────────────
// autoHeading
// ────────────────────────────────────────────

describe("autoHeading", () => {
  it("nudges heading toward travel direction", () => {
    // Current heading = 0 (north), travel = π/2 (east), offset = 0
    const h = autoHeading(0, Math.PI / 2, 0, toRad(-56));
    expect(h).toBeGreaterThan(0); // moved toward east
  });

  it("includes heading offset", () => {
    // Current heading = π/2 (east), travel = π/2, offset = π/4
    // Desired = π/2 + π/4 = 3π/4 → should nudge toward 3π/4
    const h = autoHeading(Math.PI / 2, Math.PI / 2, Math.PI / 4, toRad(-56));
    expect(h).toBeGreaterThan(Math.PI / 2);
  });

  it("no adjustment at top-down pitch", () => {
    const h = autoHeading(0, Math.PI / 2, 0, toRad(-90));
    expect(h).toBeCloseTo(0); // no change
  });

  it("no adjustment at pitchZero boundary", () => {
    const h = autoHeading(0, Math.PI / 2, 0, toRad(-80));
    expect(h).toBeCloseTo(0);
  });

  it("full adjustment above pitchFull", () => {
    const h1 = autoHeading(0, Math.PI / 2, 0, toRad(-56), { lerpFactor: 0.5 });
    const h2 = autoHeading(0, Math.PI / 2, 0, toRad(-45), { lerpFactor: 0.5 });
    // Both above pitchFull=-60°, so t=1, same adjustment
    expect(h1).toBeCloseTo(h2, 5);
  });

  it("partial adjustment between pitchZero and pitchFull", () => {
    // pitch = -70° is midway between -80 and -60 → t = 0.5
    const hFull = autoHeading(0, Math.PI / 2, 0, toRad(-56), { lerpFactor: 0.1 });
    const hHalf = autoHeading(0, Math.PI / 2, 0, toRad(-70), { lerpFactor: 0.1 });
    // hHalf should be about half of hFull's adjustment
    expect(hHalf).toBeGreaterThan(0);
    expect(hHalf).toBeLessThan(hFull);
    expect(hHalf).toBeCloseTo(hFull * 0.5, 3);
  });

  it("takes shortest path across ±π", () => {
    // Current = 170°, travel direction = -170° (= 190°)
    // Shortest path is +20° (not -340°)
    const h = autoHeading(toRad(170), toRad(-170), 0, toRad(-56), { lerpFactor: 1.0 });
    expect(h).toBeGreaterThan(toRad(170)); // nudged positive
  });

  it("returns current heading when lerp is 0", () => {
    const h = autoHeading(1.0, 2.0, 0, toRad(-56), { lerpFactor: 0 });
    expect(h).toBeCloseTo(1.0);
  });

  // ── Multi-frame convergence direction tests ──

  it("converges in correct direction for right turn (north→east)", () => {
    // Simulate 100 frames of heading converging toward east (π/2)
    let heading = 0; // facing north
    const travelBearing = Math.PI / 2; // travel east
    const opts = { lerpFactor: 0.05 };
    for (let i = 0; i < 100; i++) {
      heading = autoHeading(heading, travelBearing, 0, toRad(-56), opts);
    }
    // Should converge close to π/2, not diverge or go opposite
    expect(heading).toBeGreaterThan(Math.PI / 4);
    expect(heading).toBeLessThan(Math.PI); // not overshoot
  });

  it("converges in correct direction for left turn (north→west)", () => {
    let heading = 0;
    const travelBearing = -Math.PI / 2; // travel west
    const opts = { lerpFactor: 0.05 };
    for (let i = 0; i < 100; i++) {
      heading = autoHeading(heading, travelBearing, 0, toRad(-56), opts);
    }
    // Should converge toward -π/2 (west)
    expect(heading).toBeLessThan(-Math.PI / 4);
    expect(heading).toBeGreaterThan(-Math.PI);
  });

  it("converges correctly for U-turn (north→south)", () => {
    let heading = 0;
    const travelBearing = Math.PI; // travel south
    const opts = { lerpFactor: 0.05 };
    for (let i = 0; i < 200; i++) {
      heading = autoHeading(heading, travelBearing, 0, toRad(-56), opts);
    }
    // Should converge to ±π (south)
    expect(Math.abs(heading)).toBeGreaterThan(Math.PI * 0.8);
  });

  it("converges when starting from CesiumJS-range heading (5.5 rad → north)", () => {
    // CesiumJS returns heading in [0, 2π). Test with heading near 2π (≈north).
    let heading = 5.5; // ≈315° (NW)
    const travelBearing = 0; // travel north
    const opts = { lerpFactor: 0.05 };
    for (let i = 0; i < 100; i++) {
      heading = autoHeading(heading, travelBearing, 0, toRad(-56), opts);
    }
    // Should converge toward 0 (or 2π)
    // angleDiff(0, 5.5) ≈ 0.78, so heading should increase toward 2π/0
    const wrapped = Math.atan2(Math.sin(heading), Math.cos(heading));
    expect(Math.abs(wrapped)).toBeLessThan(0.3);
  });
});

// ────────────────────────────────────────────
// detectUserDrag
// ────────────────────────────────────────────

describe("detectUserDrag", () => {
  it("no drag when heading matches", () => {
    const r = detectUserDrag(1.0, 1.0, 0.5);
    expect(r.dragged).toBe(false);
    expect(r.headingOffset).toBe(0.5);
  });

  it("no drag for sub-threshold change", () => {
    // Default threshold is 0.01 rad
    const r = detectUserDrag(1.005, 1.0, 0.5);
    expect(r.dragged).toBe(false);
  });

  it("detects positive drag", () => {
    const r = detectUserDrag(1.1, 1.0, 0.0);
    expect(r.dragged).toBe(true);
    expect(r.headingOffset).toBeCloseTo(0.1, 2);
  });

  it("detects negative drag", () => {
    const r = detectUserDrag(0.9, 1.0, 0.0);
    expect(r.dragged).toBe(true);
    expect(r.headingOffset).toBeCloseTo(-0.1, 2);
  });

  it("accumulates offset from previous value", () => {
    const r = detectUserDrag(1.1, 1.0, 0.5);
    expect(r.dragged).toBe(true);
    expect(r.headingOffset).toBeCloseTo(0.6, 2);
  });

  it("wraps offset to [-π, π]", () => {
    const r = detectUserDrag(0.5, 0.0, Math.PI - 0.1);
    expect(r.dragged).toBe(true);
    // offset + 0.5 = π + 0.4, should wrap to -(π - 0.4)
    expect(Math.abs(r.headingOffset)).toBeLessThanOrEqual(Math.PI);
  });

  it("respects custom threshold", () => {
    const r = detectUserDrag(1.01, 1.0, 0.0, 0.1);
    expect(r.dragged).toBe(false); // below 0.1 threshold
  });
});

// ────────────────────────────────────────────
// estimateFrameSpeed
// ────────────────────────────────────────────

describe("estimateFrameSpeed", () => {
  it("returns 0 when no previous position", () => {
    expect(estimateFrameSpeed(null, null, 0, 0)).toBe(0);
  });

  it("returns 0 for same position", () => {
    expect(estimateFrameSpeed(toRad(139), toRad(35), toRad(139), toRad(35))).toBeCloseTo(0);
  });

  it("computes distance for east movement at equator", () => {
    // 0.001 rad ≈ 6.371 km at equator
    const d = estimateFrameSpeed(0, 0, 0.001, 0);
    expect(d).toBeCloseTo(6371, -1);
  });

  it("computes smaller distance at higher latitude", () => {
    const dEquator = estimateFrameSpeed(0, 0, 0.001, 0);
    const dHighLat = estimateFrameSpeed(0, toRad(60), 0.001, toRad(60));
    expect(dHighLat).toBeLessThan(dEquator);
    // At 60°, cos(60°) = 0.5 → half the east-west distance
    expect(dHighLat).toBeCloseTo(dEquator * 0.5, -1);
  });

  it("returns correct distance for north movement", () => {
    const d = estimateFrameSpeed(0, 0, 0, 0.001);
    expect(d).toBeCloseTo(6371, -1);
  });
});

// ────────────────────────────────────────────
// computeNarrowFov
// ────────────────────────────────────────────

describe("computeNarrowFov", () => {
  it("returns vertical FOV for landscape (aspect ≥ 1)", () => {
    const vfov = Math.PI / 3; // 60°
    expect(computeNarrowFov(vfov, 16 / 9)).toBe(vfov);
    expect(computeNarrowFov(vfov, 1)).toBe(vfov);
  });

  it("returns narrower horizontal FOV for portrait", () => {
    const vfov = Math.PI / 3;
    const narrow = computeNarrowFov(vfov, 0.5);
    // hfov = 2*atan(0.5 * tan(30°)) ≈ 32.2°
    expect(narrow).toBeLessThan(vfov);
    expect(narrow).toBeCloseTo(
      2 * Math.atan(0.5 * Math.tan(vfov / 2)),
      5,
    );
  });

  it("portrait 9:16 gives narrower FOV than landscape 16:9", () => {
    const vfov = Math.PI / 3;
    const landscape = computeNarrowFov(vfov, 16 / 9);
    const portrait = computeNarrowFov(vfov, 9 / 16);
    expect(portrait).toBeLessThan(landscape);
  });

  it("square aspect returns vertical FOV", () => {
    const vfov = Math.PI / 3;
    expect(computeNarrowFov(vfov, 1)).toBe(vfov);
  });

  it("very narrow portrait (phone held upright)", () => {
    const vfov = Math.PI / 3;
    const narrow = computeNarrowFov(vfov, 9 / 21); // ~0.43
    expect(narrow).toBeLessThan(Math.PI / 6); // less than 30°
  });
});

// ────────────────────────────────────────────
// computeLagDistance
// ────────────────────────────────────────────

describe("computeLagDistance", () => {
  it("returns 0 for zero speed", () => {
    expect(computeLagDistance(0, 0.08)).toBe(0);
  });

  it("returns 0 for invalid lerp factor", () => {
    expect(computeLagDistance(10, 0)).toBe(0);
    expect(computeLagDistance(10, 1)).toBe(0);
    expect(computeLagDistance(10, -0.1)).toBe(0);
  });

  it("computes speed / lerpFactor", () => {
    expect(computeLagDistance(100, 0.1)).toBe(1000);
    expect(computeLagDistance(200, 0.08)).toBe(2500);
  });

  it("higher lerp → less lag", () => {
    const lag1 = computeLagDistance(100, 0.05);
    const lag2 = computeLagDistance(100, 0.2);
    expect(lag2).toBeLessThan(lag1);
  });

  it("airplane at 50x: lag = 2600m", () => {
    // 250 m/s × 50x / 60fps ≈ 208 m/frame, lerp=0.08
    const lag = computeLagDistance(208, 0.08);
    expect(lag).toBe(2600);
  });

  it("walking at 50x: lag = 15m", () => {
    // 5 km/h × 50x / 60fps ≈ 1.2 m/frame
    const lag = computeLagDistance(1.2, 0.08);
    expect(lag).toBe(15);
  });
});

// ────────────────────────────────────────────
// computeVisibilityRange
// ────────────────────────────────────────────

describe("computeVisibilityRange", () => {
  it("returns 0 for zero lag", () => {
    expect(computeVisibilityRange(0)).toBe(0);
  });

  it("returns range where lag fits within FOV margin", () => {
    const lag = 1000;
    const fov = Math.PI / 3;
    const margin = 0.7;
    const range = computeVisibilityRange(lag, {
      fovRadians: fov,
      marginFraction: margin,
    });
    // Verify: at this range, lag/range === sin(fov * margin / 2)
    const halfAngle = (fov * margin) / 2;
    expect(lag / range).toBeCloseTo(Math.sin(halfAngle), 5);
  });

  it("wider FOV → smaller range needed", () => {
    const lag = 1000;
    const narrow = computeVisibilityRange(lag, { fovRadians: Math.PI / 6 });
    const wide = computeVisibilityRange(lag, { fovRadians: Math.PI / 2 });
    expect(wide).toBeLessThan(narrow);
  });

  it("larger margin → smaller range (entity can be closer to edge)", () => {
    const lag = 1000;
    const tight = computeVisibilityRange(lag, { marginFraction: 0.5 });
    const loose = computeVisibilityRange(lag, { marginFraction: 0.9 });
    expect(loose).toBeLessThan(tight);
  });

  it("airplane at 50x: range ≈ 7.3km", () => {
    const lag = 2600; // 208 m/frame / 0.08
    const range = computeVisibilityRange(lag);
    expect(range).toBeGreaterThan(6000);
    expect(range).toBeLessThan(9000);
  });

  it("walking at 50x: range ≈ 42m (below baseRange)", () => {
    const lag = 15;
    const range = computeVisibilityRange(lag);
    expect(range).toBeLessThan(50);
  });

  it("entity within viewport at computed range (parameterized)", () => {
    const fov = Math.PI / 3;
    const margin = 0.7;
    for (const lag of [10, 100, 1000, 5000, 10000]) {
      const range = computeVisibilityRange(lag, {
        fovRadians: fov,
        marginFraction: margin,
      });
      const halfAngle = (fov * margin) / 2;
      expect(lag / range).toBeLessThanOrEqual(Math.sin(halfAngle) + 1e-10);
    }
  });

  it("portrait viewport needs larger range", () => {
    const lag = 1000;
    const landscapeRange = computeVisibilityRange(lag, {
      fovRadians: Math.PI / 3,
    });
    // Portrait: 9:16, narrower horizontal FOV
    const portraitFov = 2 * Math.atan((9 / 16) * Math.tan(Math.PI / 6));
    const portraitRange = computeVisibilityRange(lag, {
      fovRadians: portraitFov,
    });
    expect(portraitRange).toBeGreaterThan(landscapeRange);
  });
});

// ────────────────────────────────────────────
// computeTargetRange (visibility-aware)
// ────────────────────────────────────────────

describe("computeTargetRange", () => {
  it("returns baseRange for zero speed", () => {
    expect(computeTargetRange(0)).toBe(200);
  });

  it("returns minRange when result would be below", () => {
    expect(computeTargetRange(0, { minRange: 300 })).toBe(300);
  });

  it("walking speed stays at baseRange", () => {
    // 1.2 m/frame → lag=15m → visRange≈42m < baseRange(200)
    const r = computeTargetRange(1.2);
    expect(r).toBe(200);
  });

  it("driving speed zooms out", () => {
    // 11.6 m/frame → lag=145m → visRange≈405m
    const r = computeTargetRange(11.6);
    expect(r).toBeGreaterThan(350);
    expect(r).toBeLessThan(500);
  });

  it("airplane speed zooms out significantly", () => {
    // 208 m/frame → lag=2600m → visRange≈7263m
    const r = computeTargetRange(208);
    expect(r).toBeGreaterThan(6000);
    expect(r).toBeLessThan(9000);
  });

  it("caps at maxRange", () => {
    expect(computeTargetRange(1000, { maxRange: 5000 })).toBe(5000);
  });

  it("narrower FOV requires larger range", () => {
    const wide = computeTargetRange(100, { fovRadians: Math.PI / 2 });
    const narrow = computeTargetRange(100, { fovRadians: Math.PI / 6 });
    expect(narrow).toBeGreaterThan(wide);
  });

  it("higher lerp factor reduces required range", () => {
    const slow = computeTargetRange(100, { lerpFactor: 0.05 });
    const fast = computeTargetRange(100, { lerpFactor: 0.2 });
    expect(fast).toBeLessThan(slow);
  });

  it("portrait viewport requires larger range", () => {
    const portraitFov = 2 * Math.atan((9 / 16) * Math.tan(Math.PI / 6));
    const r = computeTargetRange(100, { fovRadians: portraitFov });
    const rDefault = computeTargetRange(100);
    expect(r).toBeGreaterThan(rDefault);
  });
});

// ────────────────────────────────────────────
// lerpRange
// ────────────────────────────────────────────

describe("lerpRange", () => {
  it("stays at current if already at target", () => {
    expect(lerpRange(500, 500)).toBe(500);
  });

  it("moves toward target", () => {
    const r = lerpRange(200, 800);
    expect(r).toBeGreaterThan(200);
    expect(r).toBeLessThan(800);
  });

  it("converges after many iterations", () => {
    let range = 200;
    for (let i = 0; i < 5000; i++) {
      range = lerpRange(range, 800);
    }
    expect(range).toBeCloseTo(800, 0);
  });

  it("respects custom factor", () => {
    const slow = lerpRange(200, 800, 0.001);
    const fast = lerpRange(200, 800, 0.1);
    expect(fast).toBeGreaterThan(slow);
  });

  it("approaches from above", () => {
    const r = lerpRange(1000, 300);
    expect(r).toBeLessThan(1000);
    expect(r).toBeGreaterThan(300);
  });
});

// ────────────────────────────────────────────
// Follow simulation — end-to-end visibility
// ────────────────────────────────────────────

describe("follow simulation — visibility", () => {
  // Generate straight-line track at constant speed
  function generateTrack(
    speedMs: number,
    durationSec: number,
    startLon: number,
    startLat: number,
    heading: number,
  ): Array<{ lon: number; lat: number; timeMs: number }> {
    const R = 6371000;
    const positions: Array<{ lon: number; lat: number; timeMs: number }> = [];
    for (let t = 0; t <= durationSec; t++) {
      const dist = speedMs * t;
      const lon =
        startLon + (dist * Math.sin(heading)) / (R * Math.cos(startLat));
      const lat = startLat + (dist * Math.cos(heading)) / R;
      positions.push({ lon, lat, timeMs: t * 1000 });
    }
    return positions;
  }

  // Interpolate position at a given simulation time
  function interpAt(
    positions: Array<{ lon: number; lat: number; timeMs: number }>,
    timeMs: number,
  ): { lon: number; lat: number } {
    if (timeMs <= positions[0].timeMs) return positions[0];
    const last = positions[positions.length - 1];
    if (timeMs >= last.timeMs) return last;

    let lo = 0;
    let hi = positions.length - 1;
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (positions[mid].timeMs <= timeMs) lo = mid;
      else hi = mid;
    }
    const a = positions[lo];
    const b = positions[hi];
    const t = (timeMs - a.timeMs) / (b.timeMs - a.timeMs);
    return {
      lon: a.lon + (b.lon - a.lon) * t,
      lat: a.lat + (b.lat - a.lat) * t,
    };
  }

  // Simulate camera follow loop: EMA smoothing + visibility-based range
  function simulateFollow(
    positions: Array<{ lon: number; lat: number; timeMs: number }>,
    config: {
      playbackSpeed: number;
      fps: number;
      hLerp: number;
      fovRadians: number;
      marginFraction: number;
    },
  ) {
    const { playbackSpeed, fps, hLerp, fovRadians, marginFraction } = config;
    const frameDurationMs = (playbackSpeed * 1000) / fps;

    let smoothedLon = positions[0].lon;
    let smoothedLat = positions[0].lat;
    let range = 200;
    let totalFrames = 0;
    let offScreenFrames = 0;
    let maxLag = 0;
    let maxRange = 0;

    const startMs = positions[0].timeMs;
    const endMs = positions[positions.length - 1].timeMs;

    for (let simMs = startMs; simMs <= endMs; simMs += frameDurationMs) {
      const target = interpAt(positions, simMs);

      // EMA smoothing
      smoothedLon += (target.lon - smoothedLon) * hLerp;
      smoothedLat += (target.lat - smoothedLat) * hLerp;

      // Actual lag in meters
      const lag = estimateFrameSpeed(
        smoothedLon,
        smoothedLat,
        target.lon,
        target.lat,
      );
      if (lag > maxLag) maxLag = lag;

      // Visibility-based range with hard floor
      const visMin = computeVisibilityRange(lag, {
        fovRadians,
        marginFraction,
      });
      const rangeTarget = Math.max(200, visMin);
      range = lerpRange(range, rangeTarget);
      range = Math.max(range, visMin); // hard floor
      if (range > maxRange) maxRange = range;

      // Check visibility: is entity within FOV?
      const halfFov = fovRadians / 2;
      if (range > 0 && lag / range > Math.sin(halfFov)) {
        offScreenFrames++;
      }
      totalFrames++;
    }
    return { totalFrames, offScreenFrames, maxLag, maxRange };
  }

  const defaultConfig = {
    playbackSpeed: 50,
    fps: 60,
    hLerp: 0.08,
    fovRadians: Math.PI / 3,
    marginFraction: 0.7,
  };

  it("airplane at 50x: zero off-screen frames", () => {
    const track = generateTrack(
      250, 300, toRad(139), toRad(40), toRad(190),
    );
    const result = simulateFollow(track, defaultConfig);
    expect(result.offScreenFrames).toBe(0);
    expect(result.totalFrames).toBeGreaterThan(100);
    expect(result.maxLag).toBeGreaterThan(1000);
    expect(result.maxRange).toBeGreaterThan(5000);
  });

  it("walking at 50x: all on screen, small range", () => {
    const track = generateTrack(
      1.4, 300, toRad(139), toRad(35), toRad(45),
    );
    const result = simulateFollow(track, defaultConfig);
    expect(result.offScreenFrames).toBe(0);
    expect(result.maxRange).toBeLessThan(500);
  });

  it("driving at 50x: all on screen", () => {
    const track = generateTrack(
      13.9, 300, toRad(139), toRad(35), toRad(90),
    );
    const result = simulateFollow(track, defaultConfig);
    expect(result.offScreenFrames).toBe(0);
  });

  it("airplane at 100x: all on screen", () => {
    const track = generateTrack(
      250, 300, toRad(139), toRad(40), toRad(190),
    );
    const result = simulateFollow(track, {
      ...defaultConfig,
      playbackSpeed: 100,
    });
    expect(result.offScreenFrames).toBe(0);
    expect(result.maxRange).toBeGreaterThan(10000);
  });

  it("portrait viewport: larger range needed, still on screen", () => {
    const portraitFov = 2 * Math.atan((9 / 16) * Math.tan(Math.PI / 6));
    const track = generateTrack(
      250, 300, toRad(139), toRad(40), toRad(190),
    );
    const portraitResult = simulateFollow(track, {
      ...defaultConfig,
      fovRadians: portraitFov,
    });
    const landscapeResult = simulateFollow(track, defaultConfig);
    expect(portraitResult.offScreenFrames).toBe(0);
    expect(portraitResult.maxRange).toBeGreaterThan(landscapeResult.maxRange);
  });

  it("sudden speed change: walk → airplane, all on screen", () => {
    const walk = generateTrack(
      1.4, 60, toRad(139), toRad(35), toRad(0),
    );
    const lastWalk = walk[walk.length - 1];
    const fly = generateTrack(250, 240, lastWalk.lon, lastWalk.lat, toRad(190));
    const combined = [
      ...walk,
      ...fly.slice(1).map((p) => ({ ...p, timeMs: p.timeMs + 60000 })),
    ];
    const result = simulateFollow(combined, defaultConfig);
    expect(result.offScreenFrames).toBe(0);
  });

  it("range converges at constant speed", () => {
    const track = generateTrack(
      250, 600, toRad(139), toRad(40), toRad(190),
    );
    const result = simulateFollow(track, defaultConfig);
    // After 600s, range should have stabilized
    // Verify range is within reasonable bounds for 250 m/s at 50x
    const expectedLag = computeLagDistance(250 * 50 / 60, 0.08);
    const expectedRange = computeVisibilityRange(expectedLag, {
      fovRadians: Math.PI / 3,
      marginFraction: 0.7,
    });
    expect(result.maxRange).toBeGreaterThan(expectedRange * 0.8);
    expect(result.maxRange).toBeLessThan(expectedRange * 1.5);
  });
});

// ────────────────────────────────────────────
// Real flight data simulation (Chitose→Tsukuba cruise)
// ────────────────────────────────────────────

describe("real flight data — visibility", () => {
  // Load 300 GPS/FLP fix records from airplane cruising phase
  // (~200 m/s, Hokkaido region, lat 42.5→42.1, ~176 seconds)
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const cruiseFixes: Array<{
    timeMs: number;
    lat: number;
    lon: number;
    speed: number;
  }> = require("./__fixtures__/cruise_fixes.json");

  // Convert degrees to radians for simulation
  const track = cruiseFixes.map((f) => ({
    lon: toRad(f.lon),
    lat: toRad(f.lat),
    timeMs: f.timeMs,
  }));

  // Reuse simulation infrastructure from above
  function interpAt(
    positions: Array<{ lon: number; lat: number; timeMs: number }>,
    timeMs: number,
  ): { lon: number; lat: number } {
    if (timeMs <= positions[0].timeMs) return positions[0];
    const last = positions[positions.length - 1];
    if (timeMs >= last.timeMs) return last;
    let lo = 0;
    let hi = positions.length - 1;
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1;
      if (positions[mid].timeMs <= timeMs) lo = mid;
      else hi = mid;
    }
    const a = positions[lo];
    const b = positions[hi];
    const t = (timeMs - a.timeMs) / (b.timeMs - a.timeMs);
    return {
      lon: a.lon + (b.lon - a.lon) * t,
      lat: a.lat + (b.lat - a.lat) * t,
    };
  }

  function simReal(config: {
    playbackSpeed: number;
    fps: number;
    hLerp: number;
    fovRadians: number;
    marginFraction: number;
  }) {
    const { playbackSpeed, fps, hLerp, fovRadians, marginFraction } = config;
    const frameDurationMs = (playbackSpeed * 1000) / fps;
    let smoothedLon = track[0].lon;
    let smoothedLat = track[0].lat;
    let range = 200;
    let totalFrames = 0;
    let offScreenFrames = 0;
    let maxLag = 0;
    let maxRange = 0;
    const startMs = track[0].timeMs;
    const endMs = track[track.length - 1].timeMs;

    for (let simMs = startMs; simMs <= endMs; simMs += frameDurationMs) {
      const target = interpAt(track, simMs);
      smoothedLon += (target.lon - smoothedLon) * hLerp;
      smoothedLat += (target.lat - smoothedLat) * hLerp;
      const lag = estimateFrameSpeed(
        smoothedLon, smoothedLat, target.lon, target.lat,
      );
      if (lag > maxLag) maxLag = lag;
      const visMin = computeVisibilityRange(lag, { fovRadians, marginFraction });
      const rangeTarget = Math.max(200, visMin);
      range = lerpRange(range, rangeTarget);
      range = Math.max(range, visMin);
      if (range > maxRange) maxRange = range;
      const halfFov = fovRadians / 2;
      if (range > 0 && lag / range > Math.sin(halfFov)) {
        offScreenFrames++;
      }
      totalFrames++;
    }
    return { totalFrames, offScreenFrames, maxLag, maxRange };
  }

  it("loads fixture data correctly", () => {
    expect(cruiseFixes.length).toBe(300);
    expect(cruiseFixes[0].speed).toBeGreaterThan(200);
    expect(cruiseFixes[0].lat).toBeGreaterThan(42);
    expect(cruiseFixes[0].lon).toBeCloseTo(141.77, 0);
  });

  it("50x playback: zero off-screen frames", () => {
    const result = simReal({
      playbackSpeed: 50,
      fps: 60,
      hLerp: 0.08,
      fovRadians: Math.PI / 3,
      marginFraction: 0.7,
    });
    expect(result.offScreenFrames).toBe(0);
    expect(result.totalFrames).toBeGreaterThan(100);
    // Airplane at ~210 m/s, 50x → ~175 m/frame
    expect(result.maxLag).toBeGreaterThan(500);
    expect(result.maxRange).toBeGreaterThan(3000);
  });

  it("100x playback: zero off-screen frames", () => {
    const result = simReal({
      playbackSpeed: 100,
      fps: 60,
      hLerp: 0.08,
      fovRadians: Math.PI / 3,
      marginFraction: 0.7,
    });
    expect(result.offScreenFrames).toBe(0);
    expect(result.maxRange).toBeGreaterThan(result.maxLag);
  });

  it("portrait phone (9:16): zero off-screen frames", () => {
    const portraitFov = computeNarrowFov(Math.PI / 3, 9 / 16);
    const result = simReal({
      playbackSpeed: 50,
      fps: 60,
      hLerp: 0.08,
      fovRadians: portraitFov,
      marginFraction: 0.7,
    });
    expect(result.offScreenFrames).toBe(0);
  });

  it("500x playback: zero off-screen frames", () => {
    const result = simReal({
      playbackSpeed: 500,
      fps: 60,
      hLerp: 0.08,
      fovRadians: Math.PI / 3,
      marginFraction: 0.7,
    });
    expect(result.offScreenFrames).toBe(0);
  });
});

// ────────────────────────────────────────────
// adjustPitchForTerrain
// ────────────────────────────────────────────

describe("adjustPitchForTerrain", () => {
  // ── Default behavior (gentle, airplane-friendly) ──

  it("no adjustment when camera is well above terrain", () => {
    // 100m AGL > 10m default threshold
    const r = adjustPitchForTerrain(toRad(-56), 200, 100);
    expect(r.adjusted).toBe(false);
    expect(r.pitch).toBe(toRad(-56));
  });

  it("no adjustment at 30m AGL (above default 10m threshold)", () => {
    // 30m AGL should be fine — allows near-horizontal viewing near mountains
    const r = adjustPitchForTerrain(toRad(-20), 130, 100);
    expect(r.adjusted).toBe(false);
    expect(r.pitch).toBe(toRad(-20));
  });

  it("adjusts when camera is within 10m of terrain", () => {
    // 5m AGL < 10m default threshold → adjust
    const r = adjustPitchForTerrain(toRad(-30), 105, 100);
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeLessThan(toRad(-30));
  });

  it("gentle adjustment: less than 0.5° per frame at mild deficit", () => {
    // 8m AGL → deficit = (10-8)/10 = 0.2, squared = 0.04
    const r = adjustPitchForTerrain(toRad(-30), 108, 100);
    expect(r.adjusted).toBe(true);
    const changeDeg = Math.abs(r.pitch - toRad(-30)) * 180 / Math.PI;
    expect(changeDeg).toBeLessThan(0.5);
  });

  it("stronger adjustment when deeper underground", () => {
    const r1 = adjustPitchForTerrain(toRad(-30), 108, 100); // 8m AGL
    const r2 = adjustPitchForTerrain(toRad(-30), 101, 100); // 1m AGL
    expect(r2.pitch).toBeLessThan(r1.pitch);
  });

  it("preserves near-horizontal pitch when above threshold", () => {
    // Even at -10° pitch (very horizontal), no adjustment if above terrain
    const r = adjustPitchForTerrain(toRad(-10), 120, 100);
    expect(r.adjusted).toBe(false);
    expect(r.pitch).toBe(toRad(-10));
  });

  it("never pushes pitch beyond -85° (avoids gimbal lock)", () => {
    // Camera underground — extreme deficit
    const r = adjustPitchForTerrain(toRad(-84), 50, 100);
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeGreaterThanOrEqual(toRad(-85));
  });

  it("handles underground camera", () => {
    const r = adjustPitchForTerrain(toRad(-56), 80, 100);
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeLessThan(toRad(-56));
  });

  // ── Explicit options (backwards-compatible) ──

  it("adjusts with custom minAltitude=50 (old behavior)", () => {
    // 30m AGL < 50m → adjusts
    const r = adjustPitchForTerrain(toRad(-56), 130, 100, { minAltitude: 50 });
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeLessThan(toRad(-56));
  });

  it("respects custom minAltitude", () => {
    const r = adjustPitchForTerrain(toRad(-56), 130, 100, {
      minAltitude: 20, // camera at 30m > 20m → no adjustment
    });
    expect(r.adjusted).toBe(false);
  });

  it("adjusts more aggressively with custom adjustSpeed", () => {
    const gentle = adjustPitchForTerrain(toRad(-56), 105, 100);
    const aggressive = adjustPitchForTerrain(toRad(-56), 105, 100, {
      adjustSpeed: 0.03,
    });
    expect(aggressive.pitch).toBeLessThan(gentle.pitch);
  });

  it("respects custom minPitch", () => {
    const r = adjustPitchForTerrain(toRad(-88), 50, 100, {
      minPitch: -Math.PI * 89 / 180,
    });
    expect(r.pitch).toBeGreaterThanOrEqual(toRad(-89));
  });
});

// ────────────────────────────────────────────
// checkLineOfSight
// ────────────────────────────────────────────

describe("checkLineOfSight", () => {
  const cam = { lon: 0, lat: 0, height: 400 };
  const tgt = { lon: 0.01, lat: 0, height: 100 };

  it("returns 0 for flat terrain", () => {
    const flat = () => 0;
    expect(checkLineOfSight(cam, tgt, flat)).toBe(0);
  });

  it("detects mountain blocking view", () => {
    // Mountain at midpoint that rises above the line of sight
    const mountain = (lon: number) => {
      const mid = 0.005;
      if (Math.abs(lon - mid) < 0.002) return 500; // mountain peak
      return 0;
    };
    expect(checkLineOfSight(cam, tgt, mountain)).toBeGreaterThan(0);
  });

  it("handles terrain just below LOS with margin", () => {
    // LOS at midpoint: (400 + 100) / 2 = 250m
    // Terrain at 245m → within 10m margin → occluded
    const nearMiss = () => 245;
    expect(checkLineOfSight(cam, tgt, nearMiss, { margin: 10 })).toBeGreaterThan(0);
  });

  it("passes terrain just below margin", () => {
    // LOS heights at t=1/4,2/4,3/4: 325, 250, 175m
    // Lowest LOS - margin = 175 - 10 = 165m. Terrain at 160m → clear
    const clear = () => 160;
    expect(checkLineOfSight(cam, tgt, clear, { margin: 10 })).toBe(0);
  });

  it("handles undefined terrain height", () => {
    const noData = () => undefined;
    expect(checkLineOfSight(cam, tgt, noData)).toBe(0);
  });

  it("counts multiple occluded samples", () => {
    // All terrain is very high
    const highTerrain = () => 1000;
    const n = checkLineOfSight(cam, tgt, highTerrain, { nSamples: 4 });
    expect(n).toBe(4);
  });

  it("respects nSamples option", () => {
    const high = () => 1000;
    expect(checkLineOfSight(cam, tgt, high, { nSamples: 1 })).toBe(1);
    expect(checkLineOfSight(cam, tgt, high, { nSamples: 5 })).toBe(5);
  });
});

// ────────────────────────────────────────────
// approxCameraPosition
// ────────────────────────────────────────────

describe("approxCameraPosition", () => {
  const tgtLon = toRad(139);
  const tgtLat = toRad(35);
  const tgtH = 100;

  it("places camera south when heading = 0", () => {
    const cam = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-56), 360);
    expect(cam.lon).toBeCloseTo(tgtLon, 5); // same longitude
    expect(cam.lat).toBeLessThan(tgtLat); // south of target
  });

  it("places camera west when heading = π/2", () => {
    const cam = approxCameraPosition(tgtLon, tgtLat, tgtH, Math.PI / 2, toRad(-56), 360);
    expect(cam.lon).toBeLessThan(tgtLon); // west of target
  });

  it("places camera above target", () => {
    const cam = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-56), 360);
    expect(cam.height).toBeGreaterThan(tgtH);
  });

  it("higher pitch → higher camera", () => {
    const cam1 = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-30), 360);
    const cam2 = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-70), 360);
    expect(cam2.height).toBeGreaterThan(cam1.height);
  });

  it("larger range → further from target", () => {
    const cam1 = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-56), 200);
    const cam2 = approxCameraPosition(tgtLon, tgtLat, tgtH, 0, toRad(-56), 600);
    // cam2 should be further south
    expect(cam2.lat).toBeLessThan(cam1.lat);
  });
});

// ────────────────────────────────────────────
// occlusionNudgeDirection
// ────────────────────────────────────────────

describe("occlusionNudgeDirection", () => {
  const tgtLon = toRad(139);
  const tgtLat = toRad(35);
  const tgtH = 100;

  it("returns +1 or -1", () => {
    const flat = () => 0;
    const dir = occlusionNudgeDirection(
      tgtLon, tgtLat, tgtH, 0, toRad(-56), 360, flat,
    );
    expect(Math.abs(dir)).toBe(1);
  });

  it("nudges away from mountain on one side", () => {
    // Mountain to the west of the camera-target line
    // Camera heading=0 → camera is south, looking north
    // Mountain to the west (negative lon direction)
    const mountain = (lon: number, _lat: number) => {
      if (lon < tgtLon - 0.0001) return 800; // high terrain to the west
      return 0;
    };

    const dir = occlusionNudgeDirection(
      tgtLon, tgtLat, tgtH, 0, toRad(-56), 360, mountain,
    );
    // Should nudge heading positive (away from west mountain → toward east)
    // or negative depending on which test heading has less occlusion
    expect(Math.abs(dir)).toBe(1);
  });

  it("picks clearer side when terrain is asymmetric", () => {
    // Terrain only blocks the east side (lon > tgtLon)
    const asymmetric = (lon: number) => {
      if (lon > tgtLon) return 1000;
      return 0;
    };

    const dir = occlusionNudgeDirection(
      tgtLon, tgtLat, tgtH, 0, toRad(-56), 360, asymmetric,
    );
    // heading=0: camera is south. heading+delta shifts camera west (lon decreases),
    // heading-delta shifts camera east (lon increases → high terrain).
    // So heading+delta side is clearer → return +1
    expect(dir).toBe(1);
  });

  it("returns +1 when both sides are equally clear", () => {
    const flat = () => 0;
    const dir = occlusionNudgeDirection(
      tgtLon, tgtLat, tgtH, 0, toRad(-56), 360, flat,
    );
    // Both sides have 0 occlusion, occPlus <= occMinus → +1
    expect(dir).toBe(1);
  });
});
