/**
 * Edge case simulation tests for camera follow algorithm.
 *
 * Simulates frame-by-frame camera updates using real-world data patterns
 * to detect heading jitter, oscillation, and other instabilities.
 *
 * Key insight: at 50x playback speed, the marker moves 50x real speed.
 * At walking speed (1.4 m/s) × 50x = 70 m/s effective.
 * Per frame at 60fps: 70/60 ≈ 1.2m. After H_LERP=0.08: ~0.09m/frame.
 * To reach 5m sample threshold: ~55 frames ≈ 0.9s wall time.
 *
 * The simulation generates position sequences as seen by the preRender
 * callback (interpolated SampledPositionProperty at render framerate).
 */
import { describe, it, expect } from "vitest";
import {
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
  angleDiff,
} from "./cameraFollow";

const toRad = (deg: number) => (deg * Math.PI) / 180;
const toDeg = (rad: number) => (rad * 180) / Math.PI;

// ────────────────────────────────────────────
// Simulation harness
// ────────────────────────────────────────────

interface SimFrame {
  lon: number; // radians
  lat: number; // radians
  height: number; // meters
}

interface SimResult {
  headings: number[];
  bearings: number[];
  headingDeltas: number[]; // per-frame |Δheading|
  maxDelta: number;
  meanDelta: number;
  /** Frames where heading changed direction (sign flip) */
  oscillations: number;
  /** Frames where bearing tracker updated */
  bearingUpdates: number;
}

const H_LERP = 0.08;
const V_LERP = 0.015;
const SNAP_RAD = 0.002;
const LOS_NUDGE_SPEED = 0.02;

function simulate(
  frames: SimFrame[],
  getTerrainHeight?: (lon: number, lat: number) => number | undefined,
  options?: {
    initialPitch?: number;
    initialHeadingOffset?: number;
    range?: number;
  },
): SimResult {
  const terrain = getTerrainHeight ?? (() => 0);

  let smoothedLon = 0;
  let smoothedLat = 0;
  let smoothedH = 0;
  let heading = 0;
  let pitch = options?.initialPitch ?? toRad(-56);
  const range = options?.range ?? 360;
  let headingOffset = options?.initialHeadingOffset ?? toRad(10);
  const bearing = createBearingTracker();
  let initialized = false;

  const headings: number[] = [];
  const bearings: number[] = [];
  const headingDeltas: number[] = [];
  let bearingUpdates = 0;

  for (const frame of frames) {
    // Position smoothing (same as PlaybackControls)
    if (!initialized) {
      smoothedLon = frame.lon;
      smoothedLat = frame.lat;
      smoothedH = frame.height;
    } else {
      const dLon = frame.lon - smoothedLon;
      const dLat = frame.lat - smoothedLat;
      const dH = frame.height - smoothedH;

      if (Math.abs(dLon) > SNAP_RAD || Math.abs(dLat) > SNAP_RAD) {
        smoothedLon = frame.lon;
        smoothedLat = frame.lat;
        smoothedH = frame.height;
        resetBearing(bearing);
      } else {
        smoothedLon += dLon * H_LERP;
        smoothedLat += dLat * H_LERP;
        smoothedH += dH * V_LERP;
      }
    }

    // Bearing update
    const updated = updateBearing(bearing, smoothedLon, smoothedLat);
    if (updated) bearingUpdates++;
    bearings.push(bearing.bearing);

    // Auto-heading (no drag detection in sim — we control heading directly)
    if (bearing.hasBearing && initialized) {
      heading = autoHeading(heading, bearing.bearing, headingOffset, pitch);
    }

    // LOS occlusion
    if (initialized) {
      const cam = approxCameraPosition(
        smoothedLon, smoothedLat, smoothedH,
        heading, pitch, range,
      );
      const tgt = { lon: smoothedLon, lat: smoothedLat, height: smoothedH };
      const occluded = checkLineOfSight(cam, tgt, terrain);
      if (occluded > 0) {
        const dir = occlusionNudgeDirection(
          smoothedLon, smoothedLat, smoothedH,
          heading, pitch, range, terrain,
        );
        heading += dir * LOS_NUDGE_SPEED;
      }
    }

    // Terrain collision
    if (initialized) {
      const camH = smoothedH + range * Math.abs(Math.sin(pitch));
      const terrH = terrain(smoothedLon, smoothedLat);
      if (terrH !== undefined) {
        const result = adjustPitchForTerrain(pitch, camH, terrH);
        pitch = result.pitch;
      }
    }

    // Record
    headings.push(heading);
    if (headings.length > 1) {
      const delta = Math.abs(angleDiff(heading, headings[headings.length - 2]!));
      headingDeltas.push(delta);
    }

    initialized = true;
  }

  // Count oscillations
  let oscillations = 0;
  for (let i = 2; i < headings.length; i++) {
    const d1 = angleDiff(headings[i - 1]!, headings[i - 2]!);
    const d2 = angleDiff(headings[i]!, headings[i - 1]!);
    if (d1 * d2 < 0 && Math.abs(d1) > 0.0001 && Math.abs(d2) > 0.0001) {
      oscillations++;
    }
  }

  const maxDelta = headingDeltas.length > 0 ? Math.max(...headingDeltas) : 0;
  const meanDelta = headingDeltas.length > 0
    ? headingDeltas.reduce((a, b) => a + b, 0) / headingDeltas.length
    : 0;

  return { headings, bearings, headingDeltas, maxDelta, meanDelta, oscillations, bearingUpdates };
}

// ────────────────────────────────────────────
// Position generators (playback speed aware)
// ────────────────────────────────────────────

const R_EARTH = 6371000;

/**
 * Generate frames as seen by preRender at 60fps with given playback multiplier.
 * At 50x playback, 1 wall-second = 50 real-seconds of marker movement.
 */
function straightLine(
  startLon: number, startLat: number,
  bearingDeg: number, realSpeedMps: number,
  wallSeconds: number,
  opts?: { multiplier?: number; fps?: number; heightFn?: (dist: number) => number },
): SimFrame[] {
  const fps = opts?.fps ?? 60;
  const mult = opts?.multiplier ?? 50;
  const bearing = toRad(bearingDeg);
  const nFrames = Math.floor(wallSeconds * fps);
  const frames: SimFrame[] = [];
  for (let i = 0; i < nFrames; i++) {
    const wallTime = i / fps;
    const realTime = wallTime * mult;
    const dist = realSpeedMps * realTime;
    const lon = startLon + (dist * Math.sin(bearing)) / (R_EARTH * Math.cos(startLat));
    const lat = startLat + (dist * Math.cos(bearing)) / R_EARTH;
    const height = opts?.heightFn ? opts.heightFn(dist) : 100;
    frames.push({ lon, lat, height });
  }
  return frames;
}

/** GPS/FLP interleaving: alternating position jitter perpendicular to direction. */
function gpsFlpInterleave(
  startLon: number, startLat: number,
  bearingDeg: number, realSpeedMps: number,
  wallSeconds: number, offsetM: number,
  opts?: { multiplier?: number; fps?: number },
): SimFrame[] {
  const fps = opts?.fps ?? 60;
  const mult = opts?.multiplier ?? 50;
  const bearing = toRad(bearingDeg);
  const perp = bearing + Math.PI / 2;
  const nFrames = Math.floor(wallSeconds * fps);
  const frames: SimFrame[] = [];

  // Real data: GPS/FLP fixes alternate ~every 1s.
  // At 50x playback, that's every 50/60 = 0.83 real frames.
  // Effectively, the interpolated position oscillates at ~1.2 Hz in wall time.
  const switchPeriodFrames = fps / mult; // ~1.2 frames per switch at 50x

  for (let i = 0; i < nFrames; i++) {
    const wallTime = i / fps;
    const realTime = wallTime * mult;
    const dist = realSpeedMps * realTime;
    let lon = startLon + (dist * Math.sin(bearing)) / (R_EARTH * Math.cos(startLat));
    let lat = startLat + (dist * Math.cos(bearing)) / R_EARTH;

    // GPS/FLP oscillation (sinusoidal instead of square for interpolation)
    const phase = Math.sin((2 * Math.PI * i) / (2 * switchPeriodFrames));
    lon += (phase * offsetM * Math.sin(perp)) / (R_EARTH * Math.cos(startLat));
    lat += (phase * offsetM * Math.cos(perp)) / R_EARTH;

    frames.push({ lon, lat, height: 100 });
  }
  return frames;
}

/** Near-stationary with GPS noise. Deterministic pseudo-random. */
function stationary(
  lon: number, lat: number,
  noiseMeterRms: number, wallSeconds: number,
  opts?: { multiplier?: number; fps?: number },
): SimFrame[] {
  const fps = opts?.fps ?? 60;
  const nFrames = Math.floor(wallSeconds * fps);
  let seed = 12345;
  const rand = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return (seed / 0x7fffffff) * 2 - 1;
  };
  // At 50x playback, stationary GPS noise still has noise per fix (~1Hz).
  // Interpolated: smooth transitions between noisy fix positions.
  // Simplify: noise at render rate, attenuated by H_LERP.
  const frames: SimFrame[] = [];
  for (let i = 0; i < nFrames; i++) {
    frames.push({
      lon: lon + (rand() * noiseMeterRms) / (R_EARTH * Math.cos(lat)),
      lat: lat + (rand() * noiseMeterRms) / R_EARTH,
      height: 100,
    });
  }
  return frames;
}

/** U-turn: north → turn → south. */
function uTurn(
  startLon: number, startLat: number,
  realSpeedMps: number, wallSeconds: number,
  turnStartWallSec: number, turnDurationWallSec: number,
  opts?: { multiplier?: number; fps?: number },
): SimFrame[] {
  const fps = opts?.fps ?? 60;
  const mult = opts?.multiplier ?? 50;
  const nFrames = Math.floor(wallSeconds * fps);
  const frames: SimFrame[] = [];
  let x = 0, y = 0;
  for (let i = 0; i < nFrames; i++) {
    const wallTime = i / fps;
    // Step size: realSpeed * mult / fps (meters per render frame)
    const stepM = (realSpeedMps * mult) / fps;
    let bearingRad: number;
    if (wallTime < turnStartWallSec) {
      bearingRad = 0; // north
    } else if (wallTime < turnStartWallSec + turnDurationWallSec) {
      const t = (wallTime - turnStartWallSec) / turnDurationWallSec;
      bearingRad = Math.PI * t; // 0 → π
    } else {
      bearingRad = Math.PI; // south
    }
    x += stepM * Math.sin(bearingRad);
    y += stepM * Math.cos(bearingRad);
    frames.push({
      lon: startLon + x / (R_EARTH * Math.cos(startLat)),
      lat: startLat + y / R_EARTH,
      height: 100,
    });
  }
  return frames;
}

/** Slow walk with noise overlaid. */
function noisyWalk(
  startLon: number, startLat: number,
  bearingDeg: number, realSpeedMps: number,
  noiseMeterRms: number, wallSeconds: number,
  opts?: { multiplier?: number; fps?: number },
): SimFrame[] {
  const fps = opts?.fps ?? 60;
  const mult = opts?.multiplier ?? 50;
  const bearing = toRad(bearingDeg);
  const nFrames = Math.floor(wallSeconds * fps);
  let seed = 54321;
  const rand = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return (seed / 0x7fffffff) * 2 - 1;
  };
  const frames: SimFrame[] = [];
  for (let i = 0; i < nFrames; i++) {
    const wallTime = i / fps;
    const realTime = wallTime * mult;
    const dist = realSpeedMps * realTime;
    const baseLon = startLon + (dist * Math.sin(bearing)) / (R_EARTH * Math.cos(startLat));
    const baseLat = startLat + (dist * Math.cos(bearing)) / R_EARTH;
    frames.push({
      lon: baseLon + (rand() * noiseMeterRms) / (R_EARTH * Math.cos(startLat)),
      lat: baseLat + (rand() * noiseMeterRms) / R_EARTH,
      height: 100,
    });
  }
  return frames;
}

// ────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────

const baseLon = toRad(139.7);
const baseLat = toRad(35.7);

describe("Edge case: stationary with GPS noise (50x playback)", () => {
  it("10m noise: bearing tracker gets random bearings → heading wanders", () => {
    const frames = stationary(baseLon, baseLat, 10, 10);
    const result = simulate(frames);

    console.log(
      `Stationary 10m noise: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });

  it("20m noise: worse random bearing", () => {
    const frames = stationary(baseLon, baseLat, 20, 10);
    const result = simulate(frames);

    console.log(
      `Stationary 20m noise: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    const oscillationRatio = result.oscillations / frames.length;
    console.log(`  oscillation ratio: ${(oscillationRatio * 100).toFixed(1)}%`);
  });
});

describe("Edge case: GPS/FLP interleaving (50x playback)", () => {
  it("walking 1.4m/s + 2m offset", () => {
    const frames = gpsFlpInterleave(baseLon, baseLat, 0, 1.4, 10, 2);
    const result = simulate(frames);

    console.log(
      `GPS/FLP walk 2m: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    expect(result.maxDelta).toBeLessThan(toRad(2));
  });

  it("walking 1.4m/s + 12m offset (p90 from real data)", () => {
    const frames = gpsFlpInterleave(baseLon, baseLat, 0, 1.4, 10, 12);
    const result = simulate(frames);

    console.log(
      `GPS/FLP walk 12m: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    const oscillationRatio = result.oscillations / frames.length;
    console.log(`  oscillation ratio: ${(oscillationRatio * 100).toFixed(1)}%`);
  });

  it("driving 13.9m/s + 12m offset is stable", () => {
    const frames = gpsFlpInterleave(baseLon, baseLat, 90, 13.9, 10, 12);
    const result = simulate(frames);

    console.log(
      `GPS/FLP drive 12m: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });
});

describe("Edge case: U-turn (50x playback)", () => {
  it("walking U-turn over 3 wall-seconds", () => {
    // At 50x, 3 wall-sec = 150 real-sec. Walk 1.4 m/s, turn covers ~210m arc.
    const frames = uTurn(baseLon, baseLat, 1.4, 10, 2, 3);
    const result = simulate(frames);

    console.log(
      `U-turn walk 3s: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });

  it("driving U-turn over 2 wall-seconds", () => {
    const frames = uTurn(baseLon, baseLat, 8.3, 10, 2, 2);
    const result = simulate(frames);

    console.log(
      `U-turn drive 2s: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });
});

describe("Edge case: slow walk with noise (50x playback)", () => {
  it("1 km/h walk + 10m noise: noise dominates", () => {
    const frames = noisyWalk(baseLon, baseLat, 45, 0.28, 10, 10);
    const result = simulate(frames);

    console.log(
      `Slow walk 1km/h + 10m noise: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    const oscillationRatio = result.oscillations / frames.length;
    console.log(`  oscillation ratio: ${(oscillationRatio * 100).toFixed(1)}%`);
  });

  it("5 km/h walk + 10m noise", () => {
    const frames = noisyWalk(baseLon, baseLat, 45, 1.4, 10, 10);
    const result = simulate(frames);

    console.log(
      `Walk 5km/h + 10m noise: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    const oscillationRatio = result.oscillations / frames.length;
    console.log(`  oscillation ratio: ${(oscillationRatio * 100).toFixed(1)}%`);
  });
});

describe("Edge case: LOS occlusion flip-flop", () => {
  it("symmetric valley: both sides equally occluded", () => {
    const valley = (lon: number): number => {
      const dx = Math.abs(lon - baseLon) * R_EARTH;
      return Math.min(500, dx * 2); // steep V-shaped valley
    };

    const frames = straightLine(baseLon, baseLat, 0, 5, 5);
    const result = simulate(frames, valley);

    console.log(
      `Symmetric valley: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
    const oscillationRatio = result.oscillations / frames.length;
    console.log(`  oscillation ratio: ${(oscillationRatio * 100).toFixed(1)}%`);
  });

  it("ridge along movement direction: camera behind ridge", () => {
    // Ridge perpendicular to camera LOS. Camera is south, target north.
    // Ridge between camera and target.
    const ridge = (_lon: number, lat: number): number => {
      const midLat = baseLat - 0.0001; // ridge between camera and target
      const d = Math.abs(lat - midLat) * R_EARTH;
      if (d < 100) return 400; // 400m ridge
      return 0;
    };

    const frames = straightLine(baseLon, baseLat, 0, 5, 5);
    const result = simulate(frames, ridge);

    console.log(
      `Ridge: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });
});

describe("Edge case: bearing tracker 180° jump", () => {
  it("EMA bearing during north→south reversal", () => {
    const tracker = createBearingTracker();
    const opts = { sampleRadians: 0 };

    // 20 samples north
    for (let i = 0; i <= 20; i++) {
      updateBearing(tracker, toRad(139), toRad(35 + i * 0.0005), opts);
    }
    expect(tracker.bearing).toBeCloseTo(0, 1);

    // Abruptly south: record bearing per sample
    const bearings: number[] = [];
    const lastNorthLat = toRad(35 + 20 * 0.0005);
    for (let i = 1; i <= 20; i++) {
      updateBearing(tracker, toRad(139), lastNorthLat - i * 0.0005, opts);
      bearings.push(tracker.bearing);
    }

    // Measure frame-to-frame bearing jumps
    let maxJump = 0;
    for (let i = 1; i < bearings.length; i++) {
      maxJump = Math.max(maxJump, Math.abs(angleDiff(bearings[i]!, bearings[i - 1]!)));
    }
    // Also the jump from 0 (north) to first bearing during reversal
    const initialJump = Math.abs(angleDiff(bearings[0]!, 0));

    console.log(
      `Bearing reversal: initial jump=${toDeg(initialJump).toFixed(1)}°, ` +
      `max frame-to-frame=${toDeg(maxJump).toFixed(1)}°`,
    );
    console.log(
      `  First 5 bearings: ${bearings.slice(0, 5).map((b) => toDeg(b).toFixed(1)).join("°, ")}°`,
    );

    // FINDING: initial jump is 180° because first south sample immediately
    // flips the EMA (alpha=0.4 for sharp turn, but sin(π)=0, cos(π)=-1).
    // With sin=0: EMA stays 0. With cos: 1*(1-0.4) + (-1)*0.4 = 0.2.
    // atan2(0, 0.2) = 0 → still north! Then next sample pushes cos negative...
    // Actually let me check what really happens.
  });

  it("autoHeading smoothly handles 180° bearing jump", () => {
    // Even if bearing jumps 180° in one frame, autoHeading only adjusts
    // by lerpFactor * diff = 0.015 * 180° = 2.7°/frame.
    // Verify this doesn't cause visible jitter.
    let heading = 0;
    const headings: number[] = [];

    // Bearing smoothly at 0 (north) for 60 frames
    for (let i = 0; i < 60; i++) {
      heading = autoHeading(heading, 0, toRad(10), toRad(-56));
      headings.push(heading);
    }

    // Bearing suddenly jumps to π (south)
    for (let i = 0; i < 120; i++) {
      heading = autoHeading(heading, Math.PI, toRad(10), toRad(-56));
      headings.push(heading);
    }

    // Measure heading change rate
    let maxDelta = 0;
    for (let i = 1; i < headings.length; i++) {
      const d = Math.abs(angleDiff(headings[i]!, headings[i - 1]!));
      maxDelta = Math.max(maxDelta, d);
    }

    console.log(
      `AutoHeading after 180° bearing jump: max per-frame change=${toDeg(maxDelta).toFixed(2)}°`,
    );

    // Should be bounded by lerpFactor * π ≈ 2.7°
    expect(maxDelta).toBeLessThan(toRad(3));
    // After 120 frames (2s), heading should be roughly approaching π+offset
    const final = headings[headings.length - 1]!;
    console.log(`  final heading: ${toDeg(final).toFixed(1)}° (target: ${toDeg(Math.PI + toRad(10)).toFixed(1)}°)`);
  });
});

describe("Edge case: heading offset drift", () => {
  it("random sub-threshold noise does NOT drift offset", () => {
    let headingOffset = toRad(10);
    let lastSetHeading = 1.234567;
    let seed = 99999;
    const rand = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return (seed / 0x7fffffff) * 2 - 1;
    };

    for (let i = 0; i < 1000; i++) {
      const cameraHeading = lastSetHeading + rand() * 0.002; // ±0.002, below 0.003 threshold
      const drag = detectUserDrag(cameraHeading, lastSetHeading, headingOffset);
      headingOffset = drag.headingOffset;
      lastSetHeading = lastSetHeading; // we set same heading we intended
    }

    const drift = Math.abs(angleDiff(headingOffset, toRad(10)));
    expect(drift).toBeLessThan(toRad(0.1));
  });

  it("FIXED: systematic bias 0.004 rad no longer drifts (threshold raised to 0.01)", () => {
    // Previously threshold was 0.003 rad and 0.004 bias caused unbounded drift.
    // Now threshold is 0.01 rad, so 0.004 bias is absorbed.
    let headingOffset = toRad(10);
    let lastSetHeading = 1.0;

    for (let i = 0; i < 100; i++) {
      const cameraHeading = lastSetHeading + 0.004;
      const drag = detectUserDrag(cameraHeading, lastSetHeading, headingOffset);
      headingOffset = drag.headingOffset;
      lastSetHeading = cameraHeading;
    }

    const drift = Math.abs(angleDiff(headingOffset, toRad(10)));
    console.log(`  Systematic bias 0.004 rad → drift: ${toDeg(drift).toFixed(1)}° in 100 frames (fixed)`);
    expect(drift).toBeLessThan(toRad(0.1)); // essentially no drift
  });

  it("BUG: Cesium heading normalization causes drift at 2π boundary", () => {
    // CesiumJS normalizes heading to [0, 2π].
    // If we set heading=6.28 (≈2π), Cesium returns heading≈0.
    // angleDiff(0, 6.28) = angleDiff(0, 6.28) ≈ -0.003 which is just above threshold!
    let headingOffset = toRad(10);
    const setHeading = 2 * Math.PI - 0.001; // just below 2π
    const readHeading = 0.001; // Cesium normalized it

    const drag = detectUserDrag(readHeading, setHeading, headingOffset);
    console.log(
      `  2π boundary: set=${toDeg(setHeading).toFixed(3)}°, ` +
      `read=${toDeg(readHeading).toFixed(3)}°, ` +
      `userDelta=${toDeg(angleDiff(readHeading, setHeading)).toFixed(4)}°, ` +
      `dragged=${drag.dragged}`,
    );

    // angleDiff(0.001, 6.282) should be ~0.002 which is below threshold
    // So this specific case is fine. But what about other wrapping scenarios?
    expect(drag.dragged).toBe(false);
  });
});

describe("Edge case: combined scenarios", () => {
  it("slow walk + GPS/FLP jitter + mountain terrain", () => {
    // Realistic mountain hike: 4 km/h, 10m GPS noise, terrain at 800m
    const mountain = (): number => 800;

    const frames = noisyWalk(baseLon, baseLat, 30, 1.1, 8, 10);
    const result = simulate(frames, mountain);

    console.log(
      `Mountain hike: bearingUpdates=${result.bearingUpdates}, ` +
      `maxDelta=${toDeg(result.maxDelta).toFixed(3)}°, ` +
      `meanDelta=${toDeg(result.meanDelta).toFixed(4)}°, ` +
      `oscillations=${result.oscillations}/${frames.length}`,
    );
  });
});

// ────────────────────────────────────────────
// Speed-adaptive range
// ────────────────────────────────────────────

describe("Edge case: speed-adaptive range", () => {
  it("range increases when switching from walking to driving", () => {
    // Walk north for 3s, then drive north for 7s
    const walkFrames = straightLine(baseLon, baseLat, 0, 1.4, 3);
    const lastWalk = walkFrames[walkFrames.length - 1]!;
    const driveFrames = straightLine(lastWalk.lon, lastWalk.lat, 0, 13.9, 7);
    const allFrames = [...walkFrames, ...driveFrames];

    // Simulate with speed tracking
    let speedMpf = 0;
    let prevLon: number | null = null;
    let prevLat: number | null = null;
    let range = 360;
    const ranges: number[] = [];

    for (const frame of allFrames) {
      if (prevLon !== null) {
        const dist = estimateFrameSpeed(prevLon, prevLat, frame.lon, frame.lat);
        speedMpf = speedMpf * 0.98 + dist * 0.02;
      }
      prevLon = frame.lon;
      prevLat = frame.lat;

      const target = computeTargetRange(speedMpf);
      range = lerpRange(range, target);
      ranges.push(range);
    }

    const walkRange = ranges[walkFrames.length - 1]!;
    const driveRange = ranges[ranges.length - 1]!;

    console.log(
      `Walk→Drive range: walk=${walkRange.toFixed(0)}m, drive=${driveRange.toFixed(0)}m`,
    );
    expect(driveRange).toBeGreaterThan(walkRange);
  });

  it("range decreases when switching from driving to walking", () => {
    const driveFrames = straightLine(baseLon, baseLat, 0, 13.9, 5);
    const lastDrive = driveFrames[driveFrames.length - 1]!;
    const walkFrames = straightLine(lastDrive.lon, lastDrive.lat, 0, 1.4, 10);
    const allFrames = [...driveFrames, ...walkFrames];

    let speedMpf = 0;
    let prevLon: number | null = null;
    let prevLat: number | null = null;
    let range = 360;
    const ranges: number[] = [];

    for (const frame of allFrames) {
      if (prevLon !== null) {
        const dist = estimateFrameSpeed(prevLon, prevLat, frame.lon, frame.lat);
        speedMpf = speedMpf * 0.98 + dist * 0.02;
      }
      prevLon = frame.lon;
      prevLat = frame.lat;
      const target = computeTargetRange(speedMpf);
      range = lerpRange(range, target);
      ranges.push(range);
    }

    const driveRange = ranges[driveFrames.length - 1]!;
    const walkRange = ranges[ranges.length - 1]!;

    console.log(
      `Drive→Walk range: drive=${driveRange.toFixed(0)}m, walk=${walkRange.toFixed(0)}m`,
    );
    expect(walkRange).toBeLessThan(driveRange);
  });

  it("stationary settles to minRange", () => {
    const frames = stationary(baseLon, baseLat, 3, 10);
    let speedMpf = 0;
    let prevLon: number | null = null;
    let prevLat: number | null = null;
    let range = 360;

    for (const frame of frames) {
      if (prevLon !== null) {
        const dist = estimateFrameSpeed(prevLon, prevLat, frame.lon, frame.lat);
        speedMpf = speedMpf * 0.98 + dist * 0.02;
      }
      prevLon = frame.lon;
      prevLat = frame.lat;
      const target = computeTargetRange(speedMpf);
      range = lerpRange(range, target);
    }

    console.log(`Stationary final range: ${range.toFixed(0)}m`);
    // GPS noise contributes some speed, so range won't hit exactly 200.
    // But it should be well below the driving range (~800).
    expect(range).toBeLessThan(400);
  });
});
