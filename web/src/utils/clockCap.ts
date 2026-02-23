/**
 * Maximum wall-clock delta (ms) per frame for clock advancement.
 * Frames exceeding this are treated as stalls (e.g. drawer animation,
 * heavy React reconciliation, tab switch) and the simulation advance
 * is capped.  100ms ≈ 10fps floor.
 */
export const MAX_FRAME_WALL_MS = 100;

/**
 * Cap the simulation time advance for a single clock tick to prevent
 * time jumps caused by main-thread stalls.
 *
 * When the wall-clock delta exceeds the threshold, the simulation
 * advance is limited to what would occur at the threshold frame rate.
 *
 * @param simDeltaSec Simulation time advance this tick (seconds)
 * @param wallDeltaMs Wall-clock time since last tick (milliseconds)
 * @param multiplier Clock speed multiplier
 * @param maxWallMs Maximum allowed wall-clock delta (ms)
 * @returns Simulation advance (seconds) — original or capped
 */
export function capClockAdvance(
  simDeltaSec: number,
  wallDeltaMs: number,
  multiplier: number,
  maxWallMs: number = MAX_FRAME_WALL_MS,
): number {
  if (simDeltaSec <= 0 || wallDeltaMs <= maxWallMs) return simDeltaSec;
  return (maxWallMs / 1000) * multiplier;
}
