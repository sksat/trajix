/**
 * Camera follow logic — pure functions for unit testing.
 *
 * These functions implement the camera auto-adjustment algorithms
 * used during animation playback follow mode:
 * - Adaptive EMA bearing tracking (travel direction)
 * - Auto-heading toward travel direction with pitch-dependent blend
 * - User drag detection for heading offset preservation
 * - Terrain collision pitch adjustment
 * - Line-of-sight occlusion avoidance (heading nudge)
 */

// ────────────────────────────────────────────
// Geodesic bearing
// ────────────────────────────────────────────

/** Compute initial bearing (radians) from point A to point B on a sphere. */
export function geodeticBearing(
  lonA: number,
  latA: number,
  lonB: number,
  latB: number,
): number {
  // Normalize dLon to [-π, π] to handle antimeridian crossings
  const dLon = Math.atan2(Math.sin(lonB - lonA), Math.cos(lonB - lonA));
  return Math.atan2(
    Math.sin(dLon) * Math.cos(latB),
    Math.cos(latA) * Math.sin(latB) -
      Math.sin(latA) * Math.cos(latB) * Math.cos(dLon),
  );
}

/** Shortest signed angle difference in [-π, π]. */
export function angleDiff(a: number, b: number): number {
  return Math.atan2(Math.sin(a - b), Math.cos(a - b));
}

// ────────────────────────────────────────────
// Adaptive EMA bearing tracker
// ────────────────────────────────────────────

export interface BearingTrackerState {
  sinEma: number;
  cosEma: number;
  bearing: number;
  hasBearing: boolean;
  prevLon: number | null;
  prevLat: number | null;
}

export interface BearingTrackerOptions {
  /** Min distance between samples (radians). Default: 0.00005 (~5m) */
  sampleRadians?: number;
  /** EMA alpha when direction is constant. Default: 0.05 */
  alphaMin?: number;
  /** EMA alpha on sharp turns. Default: 0.4 */
  alphaMax?: number;
  /** Angular change (radians) at which alpha reaches alphaMax. Default: π/4 */
  turnThreshold?: number;
}

export function createBearingTracker(): BearingTrackerState {
  return {
    sinEma: 0,
    cosEma: 0,
    bearing: 0,
    hasBearing: false,
    prevLon: null,
    prevLat: null,
  };
}

/**
 * Feed a new smoothed position to the bearing tracker.
 * Mutates `state` in place. Returns whether bearing was updated.
 */
export function updateBearing(
  state: BearingTrackerState,
  lon: number,
  lat: number,
  opts?: BearingTrackerOptions,
): boolean {
  const sampleRad = opts?.sampleRadians ?? 0.00005;
  const alphaMin = opts?.alphaMin ?? 0.05;
  const alphaMax = opts?.alphaMax ?? 0.4;
  const turnThresh = opts?.turnThreshold ?? Math.PI / 4;

  // Check if moved enough to sample
  if (
    state.prevLon !== null &&
    Math.abs(lon - state.prevLon) <= sampleRad &&
    Math.abs(lat - state.prevLat!) <= sampleRad
  ) {
    return false;
  }

  let updated = false;

  if (state.prevLon !== null) {
    const inst = geodeticBearing(state.prevLon, state.prevLat!, lon, lat);

    if (!state.hasBearing) {
      state.sinEma = Math.sin(inst);
      state.cosEma = Math.cos(inst);
      state.hasBearing = true;
    } else {
      const cur = Math.atan2(state.sinEma, state.cosEma);
      const delta = Math.abs(angleDiff(inst, cur));
      const alpha =
        alphaMin + (alphaMax - alphaMin) * Math.min(1, delta / turnThresh);
      state.sinEma = state.sinEma * (1 - alpha) + Math.sin(inst) * alpha;
      state.cosEma = state.cosEma * (1 - alpha) + Math.cos(inst) * alpha;
    }
    state.bearing = Math.atan2(state.sinEma, state.cosEma);
    updated = true;
  }

  state.prevLon = lon;
  state.prevLat = lat;
  return updated;
}

/** Reset tracker (e.g. after seek jump). */
export function resetBearing(state: BearingTrackerState): void {
  state.sinEma = 0;
  state.cosEma = 0;
  state.bearing = 0;
  state.hasBearing = false;
  state.prevLon = null;
  state.prevLat = null;
}

// ────────────────────────────────────────────
// Auto-heading toward travel direction
// ────────────────────────────────────────────

export interface AutoHeadingOptions {
  /** Per-frame lerp factor. Default: 0.015 */
  lerpFactor?: number;
  /** Pitch (radians) at which auto-heading is fully active. Default: toRad(-60) */
  pitchFull?: number;
  /** Pitch (radians) at which auto-heading is fully disabled. Default: toRad(-80) */
  pitchZero?: number;
}

/**
 * Compute the heading adjustment toward the travel direction.
 * Returns the new heading after applying the lerp.
 */
export function autoHeading(
  currentHeading: number,
  travelBearing: number,
  headingOffset: number,
  pitch: number,
  opts?: AutoHeadingOptions,
): number {
  const lerpFactor = opts?.lerpFactor ?? 0.015;
  const pitchFull = opts?.pitchFull ?? -Math.PI * 60 / 180;
  const pitchZero = opts?.pitchZero ?? -Math.PI * 80 / 180;

  // Pitch-dependent blend: 0 near top-down, 1 at moderate pitch
  const t = Math.max(
    0,
    Math.min(1, (pitch - pitchZero) / (pitchFull - pitchZero)),
  );
  if (t === 0) return currentHeading;

  const desired = travelBearing + headingOffset;
  const diff = angleDiff(desired, currentHeading);
  return currentHeading + diff * lerpFactor * t;
}

// ────────────────────────────────────────────
// User drag detection
// ────────────────────────────────────────────

export interface DragDetectResult {
  headingOffset: number;
  dragged: boolean;
}

/**
 * Detect if the user dragged the camera heading and update the offset.
 * Compares actual camera heading with what we set last frame.
 */
export function detectUserDrag(
  cameraHeading: number,
  lastSetHeading: number,
  currentOffset: number,
  threshold?: number,
): DragDetectResult {
  const thresh = threshold ?? 0.01;
  const userDelta = angleDiff(cameraHeading, lastSetHeading);

  if (Math.abs(userDelta) > thresh) {
    const newOffset = Math.atan2(
      Math.sin(currentOffset + userDelta),
      Math.cos(currentOffset + userDelta),
    );
    return { headingOffset: newOffset, dragged: true };
  }

  return { headingOffset: currentOffset, dragged: false };
}

// ────────────────────────────────────────────
// Speed-adaptive camera range
// ────────────────────────────────────────────

/**
 * Estimate per-frame speed (meters) from consecutive raw positions.
 * Use with EMA for smooth tracking.
 */
export function estimateFrameSpeed(
  prevLon: number | null,
  prevLat: number | null,
  lon: number,
  lat: number,
): number {
  if (prevLon === null || prevLat === null) return 0;
  const R = 6371000;
  const dX = (lon - prevLon) * R * Math.cos(lat);
  const dY = (lat - prevLat) * R;
  return Math.sqrt(dX * dX + dY * dY);
}

// ────────────────────────────────────────────
// Viewport-aware visibility
// ────────────────────────────────────────────

/**
 * Compute the narrower FOV axis for a perspective camera.
 * Landscape (aspect ≥ 1): vertical FOV is narrower.
 * Portrait (aspect < 1): horizontal FOV is narrower.
 * Injectable for responsive testing.
 */
export function computeNarrowFov(
  verticalFov: number,
  aspectRatio: number,
): number {
  if (aspectRatio >= 1) return verticalFov;
  return 2 * Math.atan(aspectRatio * Math.tan(verticalFov / 2));
}

/**
 * Compute steady-state lag distance (meters) for an EMA position smoother.
 * When an entity moves at constant velocity v per frame with EMA factor α,
 * the smoothed position lags by v/α meters at steady state.
 */
export function computeLagDistance(
  speedPerFrame: number,
  lerpFactor: number,
): number {
  if (lerpFactor <= 0 || lerpFactor >= 1 || speedPerFrame <= 0) return 0;
  return speedPerFrame / lerpFactor;
}

/**
 * Compute minimum camera range to keep a point at `lagDistance` meters
 * from the camera center within the viewport.
 *
 * Model: angular subtense of lag from camera ≈ lag / range.
 * For visibility: lag / range < sin(fov * margin / 2).
 * So: range > lag / sin(fov * margin / 2).
 *
 * Parameters injectable for responsive/viewport testing.
 */
export function computeVisibilityRange(
  lagDistance: number,
  opts?: {
    fovRadians?: number;
    marginFraction?: number;
  },
): number {
  if (lagDistance <= 0) return 0;
  const fov = opts?.fovRadians ?? Math.PI / 3;
  const margin = opts?.marginFraction ?? 0.7;
  const halfAngle = (fov * margin) / 2;
  return lagDistance / Math.sin(halfAngle);
}

/**
 * Compute target camera range from speed and viewport parameters.
 * Uses visibility-aware calculation: range must be large enough that
 * the EMA-smoothed lag keeps the entity within the viewport.
 */
export function computeTargetRange(
  speedMpf: number,
  opts?: {
    baseRange?: number;
    minRange?: number;
    maxRange?: number;
    lerpFactor?: number;
    fovRadians?: number;
    marginFraction?: number;
  },
): number {
  const base = opts?.baseRange ?? 200;
  const min = opts?.minRange ?? 200;
  const max = opts?.maxRange ?? 100_000;
  const lerpFactor = opts?.lerpFactor ?? 0.08;
  const fov = opts?.fovRadians ?? Math.PI / 3;
  const margin = opts?.marginFraction ?? 0.7;

  const lag = computeLagDistance(speedMpf, lerpFactor);
  const visRange = computeVisibilityRange(lag, {
    fovRadians: fov,
    marginFraction: margin,
  });
  return Math.max(min, Math.min(max, Math.max(base, visRange)));
}

/**
 * Smoothly lerp camera range toward target.
 * Gentle factor preserves user zoom while slowly adapting.
 */
export function lerpRange(
  currentRange: number,
  targetRange: number,
  factor?: number,
): number {
  const f = factor ?? 0.003;
  return currentRange + (targetRange - currentRange) * f;
}

// ────────────────────────────────────────────
// Terrain collision — pitch adjustment
// ────────────────────────────────────────────

export interface TerrainCollisionResult {
  pitch: number;
  adjusted: boolean;
}

/**
 * Adjust pitch toward nadir when camera is too close to terrain.
 *
 * Uses a "hard floor" approach with gentle quadratic response:
 * - Small clearance threshold (10m default) to allow near-horizontal views
 * - Quadratic deficit for smooth response near the threshold
 * - minPitch capped at -85° to avoid gimbal lock at nadir
 *
 * Returns the new pitch and whether it was adjusted.
 */
export function adjustPitchForTerrain(
  currentPitch: number,
  cameraAltitude: number,
  terrainHeight: number,
  options?: {
    minAltitude?: number;
    adjustSpeed?: number;
    minPitch?: number;
  },
): TerrainCollisionResult {
  const minAlt = options?.minAltitude ?? 10;
  const adjustSpeed = options?.adjustSpeed ?? 0.005;
  const minPitch = options?.minPitch ?? -Math.PI * 85 / 180;

  const camAlt = cameraAltitude - terrainHeight;
  if (camAlt >= minAlt) {
    return { pitch: currentPitch, adjusted: false };
  }

  const deficit = Math.min(1, (minAlt - camAlt) / minAlt);
  // Quadratic response: gentle near threshold, stronger when deeply underground
  let newPitch = currentPitch - adjustSpeed * deficit * deficit;
  newPitch = Math.max(newPitch, minPitch);
  return { pitch: newPitch, adjusted: true };
}

// ────────────────────────────────────────────
// Line-of-sight occlusion check
// ────────────────────────────────────────────

export interface LosPoint {
  lon: number;
  lat: number;
  height: number;
}

/**
 * Check if terrain occludes the line of sight between camera and target.
 * `getTerrainHeight` is injected for testability (in production: globe.getHeight).
 *
 * Returns the number of occluded sample points (0 = clear).
 */
export function checkLineOfSight(
  camera: LosPoint,
  target: LosPoint,
  getTerrainHeight: (lon: number, lat: number) => number | undefined,
  options?: { nSamples?: number; margin?: number },
): number {
  const nSamples = options?.nSamples ?? 3;
  const margin = options?.margin ?? 10;
  let occluded = 0;

  for (let i = 1; i <= nSamples; i++) {
    const t = i / (nSamples + 1);
    const lon = camera.lon + (target.lon - camera.lon) * t;
    const lat = camera.lat + (target.lat - camera.lat) * t;
    const losH = camera.height + (target.height - camera.height) * t;
    const terrH = getTerrainHeight(lon, lat);
    if (terrH !== undefined && terrH > losH - margin) {
      occluded++;
    }
  }

  return occluded;
}

/**
 * Compute approximate camera position in Cartographic for a given heading,
 * using flat-Earth approximation (valid for range < ~1km).
 */
export function approxCameraPosition(
  targetLon: number,
  targetLat: number,
  targetHeight: number,
  heading: number,
  pitch: number,
  range: number,
): LosPoint {
  const R = 6371000;
  const hDist = range * Math.cos(pitch);
  const cosLat = Math.cos(targetLat);
  return {
    lon: targetLon - (hDist * Math.sin(heading)) / (R * cosLat),
    lat: targetLat - (hDist * Math.cos(heading)) / R,
    height: targetHeight - range * Math.sin(pitch),
  };
}

/**
 * Determine which heading direction to nudge when line of sight is occluded.
 * Tests heading ± testDelta and returns +1 or -1 for the clearer direction.
 */
export function occlusionNudgeDirection(
  targetLon: number,
  targetLat: number,
  targetHeight: number,
  heading: number,
  pitch: number,
  range: number,
  getTerrainHeight: (lon: number, lat: number) => number | undefined,
  testDelta?: number,
): number {
  const delta = testDelta ?? 0.15;

  const camPlus = approxCameraPosition(
    targetLon, targetLat, targetHeight,
    heading + delta, pitch, range,
  );
  const camMinus = approxCameraPosition(
    targetLon, targetLat, targetHeight,
    heading - delta, pitch, range,
  );

  const target: LosPoint = { lon: targetLon, lat: targetLat, height: targetHeight };

  const occPlus = checkLineOfSight(camPlus, target, getTerrainHeight, { nSamples: 2 });
  const occMinus = checkLineOfSight(camMinus, target, getTerrainHeight, { nSamples: 2 });

  return occPlus <= occMinus ? +1 : -1;
}
