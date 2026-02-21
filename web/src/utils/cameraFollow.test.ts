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
// computeTargetRange
// ────────────────────────────────────────────

describe("computeTargetRange", () => {
  it("returns baseRange for zero speed", () => {
    expect(computeTargetRange(0)).toBe(200);
  });

  it("returns minRange when result would be below", () => {
    expect(computeTargetRange(-10)).toBe(200);
  });

  it("scales with speed", () => {
    // Walking at 50x: ~1.2 m/frame → 200 + 1.2*50 = 260
    expect(computeTargetRange(1.2)).toBeCloseTo(260);
  });

  it("caps at maxRange", () => {
    expect(computeTargetRange(1000)).toBe(3000);
  });

  it("reasonable range for driving at 50x", () => {
    // 50 km/h × 50x / 60fps = 11.6 m/frame → 200 + 11.6*50 = 780
    const r = computeTargetRange(11.6);
    expect(r).toBeGreaterThan(700);
    expect(r).toBeLessThan(900);
  });

  it("respects custom options", () => {
    const r = computeTargetRange(10, {
      baseRange: 100,
      speedScale: 100,
      minRange: 100,
      maxRange: 5000,
    });
    expect(r).toBeCloseTo(1100);
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
// adjustPitchForTerrain
// ────────────────────────────────────────────

describe("adjustPitchForTerrain", () => {
  it("no adjustment when camera is above minimum", () => {
    const r = adjustPitchForTerrain(toRad(-56), 200, 100);
    expect(r.adjusted).toBe(false);
    expect(r.pitch).toBe(toRad(-56));
  });

  it("no adjustment when exactly at minimum", () => {
    const r = adjustPitchForTerrain(toRad(-56), 150, 100);
    expect(r.adjusted).toBe(false);
  });

  it("adjusts pitch when camera is below minimum altitude", () => {
    const r = adjustPitchForTerrain(toRad(-56), 130, 100);
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeLessThan(toRad(-56)); // more negative = more top-down
  });

  it("adjusts more aggressively when closer to terrain", () => {
    const r1 = adjustPitchForTerrain(toRad(-56), 140, 100); // 40m, deficit=0.2
    const r2 = adjustPitchForTerrain(toRad(-56), 110, 100); // 10m, deficit=0.8

    expect(r2.pitch).toBeLessThan(r1.pitch); // more adjustment when closer
  });

  it("clamps at minimum pitch", () => {
    // Camera way below terrain — large deficit
    const r = adjustPitchForTerrain(toRad(-88), 50, 100);
    expect(r.adjusted).toBe(true);
    expect(r.pitch).toBeGreaterThanOrEqual(toRad(-89));
  });

  it("respects custom options", () => {
    const r = adjustPitchForTerrain(toRad(-56), 130, 100, {
      minAltitude: 20, // camera at 30m > 20m → no adjustment
    });
    expect(r.adjusted).toBe(false);
  });

  it("handles negative camera altitude (underground)", () => {
    const r = adjustPitchForTerrain(toRad(-56), 80, 100); // 20m below terrain
    expect(r.adjusted).toBe(true);
    // deficit = (50 - (-20)) / 50 = 1.4 → clamped to min(1, 1.4) = 1
    expect(r.pitch).toBeLessThan(toRad(-56));
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
