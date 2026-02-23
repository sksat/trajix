/**
 * NLP fix filtering utilities: binary search, time-based active fix selection, style.
 *
 * Pure functions with no CesiumJS dependency — easy to test with vitest.
 */


export interface NlpFixEntry {
  fix: { unix_time_ms: number; accuracy_m: number | null };
  quality: "GapFallback" | "Rejected";
}

/** Default maximum linger time — how long a fix stays visible after its timestamp (ms). */
const DEFAULT_MAX_LINGER_MS = 5000;

/** Maximum gap between consecutive fixes to be considered part of the same burst (ms). */
const BURST_GAP_MS = 1000;

/**
 * Binary search for the first index where `fixes[i].fix.unix_time_ms >= targetMs`.
 * Returns `fixes.length` if all entries are before `targetMs`.
 */
export function lowerBoundNlp(
  fixes: NlpFixEntry[],
  targetMs: number,
): number {
  let lo = 0;
  let hi = fixes.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (fixes[mid]!.fix.unix_time_ms < targetMs) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Find NLP fixes that should be displayed at `currentMs`.
 *
 * Display rule:
 * 1. Find the most recent fix with timestamp <= currentMs
 * 2. If it's older than `maxLingerMs`, return [] (too stale)
 * 3. Walk backwards from that fix, collecting a "burst" —
 *    consecutive fixes within BURST_GAP_MS of each other
 * 4. Return the burst (oldest first)
 */
export function findActiveNlpFixes(
  fixes: NlpFixEntry[],
  currentMs: number,
  maxLingerMs: number = DEFAULT_MAX_LINGER_MS,
): NlpFixEntry[] {
  if (fixes.length === 0) return [];

  // Find the insertion point for currentMs + 1 (exclusive upper bound)
  // Then the most recent fix <= currentMs is at idx - 1
  const idx = lowerBoundNlp(fixes, currentMs + 1);
  if (idx === 0) return []; // all fixes are after currentMs

  const latestIdx = idx - 1;
  const latestMs = fixes[latestIdx]!.fix.unix_time_ms;

  // Check linger: if the most recent fix is too old, nothing to show
  if (currentMs - latestMs > maxLingerMs) return [];

  // Collect burst: walk backwards while gap between consecutive fixes <= BURST_GAP_MS
  let burstStart = latestIdx;
  while (burstStart > 0) {
    const gap =
      fixes[burstStart]!.fix.unix_time_ms -
      fixes[burstStart - 1]!.fix.unix_time_ms;
    if (gap > BURST_GAP_MS) break;
    burstStart--;
  }

  return fixes.slice(burstStart, latestIdx + 1);
}

/**
 * Return the height to use for NLP entity positions and line endpoints.
 * Always 0 — CesiumJS CLAMP_TO_GROUND adjusts to actual terrain surface.
 * NLP markers must never float at GPS/marker altitude.
 */
export function nlpRenderHeight(_altitudeM: number | null): number {
  return 0;
}

export interface NlpFixStyle {
  pointSize: number;
  pointAlpha: number;
  circleAlpha: number;
  hue: "orange" | "red";
}

/** Return visual style for an NLP fix based on its quality classification. */
export function nlpStyle(quality: "GapFallback" | "Rejected"): NlpFixStyle {
  if (quality === "GapFallback") {
    return { pointSize: 8, pointAlpha: 0.9, circleAlpha: 0.35, hue: "orange" };
  }
  return { pointSize: 6, pointAlpha: 0.7, circleAlpha: 0.25, hue: "red" };
}
