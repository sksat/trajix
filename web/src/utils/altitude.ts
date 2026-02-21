/**
 * Altitude spike filter using running median.
 *
 * GPS altitude has inherent vertical noise (2-3x worse than horizontal).
 * Additionally, GPS and FLP providers interleave at sub-second intervals
 * with systematically different altitudes (~1.6m median, up to ~80m worst case).
 * This creates ugly vertical pillars in 3D visualization.
 *
 * Strategy: compute running median of surrounding points, replace any point
 * that deviates more than a threshold from its local median. This naturally
 * handles both provider interleaving jitter and genuine altitude spikes.
 */

import type { FixRecord } from "../types/gnss";

export interface FilterStats {
  pointsReplaced: number;
  maxDeviation: number;
}

export interface FilterOptions {
  /** Maximum allowed deviation from local median (meters). Default: 30 */
  deviationThreshold?: number;
  /** Half-window size for median computation. Window = 2*halfWindow+1. Default: 5 */
  halfWindow?: number;
}

/**
 * Filter altitude spikes using a running median approach.
 *
 * For each point, computes the median of surrounding ±halfWindow points.
 * If the point's height deviates from the median by more than deviationThreshold,
 * it is replaced with the median value.
 *
 * @returns Filtered heights array + statistics
 */
export function filterAltitudeSpikes(
  _fixes: FixRecord[],
  heights: number[],
  options?: FilterOptions,
): { filteredHeights: number[]; stats: FilterStats } {
  const threshold = options?.deviationThreshold ?? 30;
  const halfWin = options?.halfWindow ?? 5;

  if (heights.length <= 2) {
    return {
      filteredHeights: [...heights],
      stats: { pointsReplaced: 0, maxDeviation: 0 },
    };
  }

  const n = heights.length;
  const filtered = new Array<number>(n);
  let pointsReplaced = 0;
  let maxDeviation = 0;

  for (let i = 0; i < n; i++) {
    // Collect heights in the window [i - halfWin, i + halfWin]
    const lo = Math.max(0, i - halfWin);
    const hi = Math.min(n - 1, i + halfWin);
    const window: number[] = [];
    for (let j = lo; j <= hi; j++) {
      window.push(heights[j]!);
    }

    // Compute median
    window.sort((a, b) => a - b);
    const mid = window.length >> 1;
    const median =
      window.length % 2 === 1
        ? window[mid]!
        : (window[mid - 1]! + window[mid]!) / 2;

    const deviation = Math.abs(heights[i]! - median);
    if (deviation > threshold) {
      filtered[i] = median;
      pointsReplaced++;
      if (deviation > maxDeviation) {
        maxDeviation = deviation;
      }
    } else {
      filtered[i] = heights[i]!;
    }
  }

  return {
    filteredHeights: filtered,
    stats: { pointsReplaced, maxDeviation },
  };
}
