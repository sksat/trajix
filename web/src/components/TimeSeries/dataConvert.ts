/**
 * Convert ProcessingResult epoch data to uPlot AlignedData format.
 *
 * uPlot expects [timestamps_sec[], series1[], series2[], ...]
 * where timestamps are in Unix seconds (not ms).
 */
import type uPlot from "uplot";
import type { StatusEpochJs, FixEpochJs } from "../../types/gnss";

/**
 * Convert status epochs to CN0 chart data.
 * Series: cn0_mean_all, cn0_mean_used
 */
export function statusEpochsToCn0Data(
  epochs: StatusEpochJs[],
): uPlot.AlignedData {
  const n = epochs.length;
  const times = new Float64Array(n);
  const cn0All = new Float64Array(n);
  const cn0Used = new Float64Array(n);

  for (let i = 0; i < n; i++) {
    const e = epochs[i]!;
    times[i] = e.time_ms / 1000;
    cn0All[i] = e.cn0_mean_all;
    cn0Used[i] = Number.isNaN(e.cn0_mean_used)
      ? (null as unknown as number)
      : e.cn0_mean_used;
  }

  return [times, cn0All, cn0Used];
}

/**
 * Convert status epochs to satellite count chart data.
 * Series: num_visible, num_used
 */
export function statusEpochsToSatCountData(
  epochs: StatusEpochJs[],
): uPlot.AlignedData {
  const n = epochs.length;
  const times = new Float64Array(n);
  const visible = new Float64Array(n);
  const used = new Float64Array(n);

  for (let i = 0; i < n; i++) {
    const e = epochs[i]!;
    times[i] = e.time_ms / 1000;
    visible[i] = e.num_visible;
    used[i] = e.num_used;
  }

  return [times, visible, used];
}

/**
 * Convert fix epochs to accuracy chart data.
 * Series: accuracy_m, vertical_accuracy_m
 */
export function fixEpochsToAccuracyData(
  epochs: FixEpochJs[],
): uPlot.AlignedData {
  const n = epochs.length;
  const times = new Float64Array(n);
  const horiz = new Float64Array(n);
  const vert = new Float64Array(n);

  for (let i = 0; i < n; i++) {
    const e = epochs[i]!;
    times[i] = e.time_ms / 1000;
    horiz[i] = e.accuracy_m ?? (null as unknown as number);
    vert[i] = e.vertical_accuracy_m ?? (null as unknown as number);
  }

  return [times, horiz, vert];
}

/**
 * Convert fix epochs to speed chart data.
 * Series: speed_mps
 */
export function fixEpochsToSpeedData(
  epochs: FixEpochJs[],
): uPlot.AlignedData {
  const n = epochs.length;
  const times = new Float64Array(n);
  const speed = new Float64Array(n);

  for (let i = 0; i < n; i++) {
    const e = epochs[i]!;
    times[i] = e.time_ms / 1000;
    speed[i] = e.speed_mps ?? (null as unknown as number);
  }

  return [times, speed];
}
