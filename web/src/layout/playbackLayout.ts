/**
 * Playback controls layout logic.
 *
 * On mobile (<768px), the controls split into two rows:
 *   Row 1: play + speed + time + spacer + follow
 *   Row 2: seek slider (full width)
 *
 * On desktop: single row with all elements.
 */
import { isMobile } from "./sidebarState";

export type PlaybackElement = "play" | "speed" | "seek" | "time" | "follow";

const ALL_ELEMENTS: PlaybackElement[] = [
  "play",
  "speed",
  "seek",
  "time",
  "follow",
];

const ROW2_COMPACT: Set<PlaybackElement> = new Set(["seek"]);

/** Whether to use compact (two-row) layout. */
export function isCompactLayout(viewportWidth: number): boolean {
  return isMobile(viewportWidth);
}

/** Which row an element belongs to (1 = top, 2 = bottom). */
export function elementRow(
  element: PlaybackElement,
  viewportWidth: number,
): 1 | 2 {
  if (!isMobile(viewportWidth)) return 1;
  return ROW2_COMPACT.has(element) ? 2 : 1;
}

/** Ordered elements for a given row. */
export function rowElements(
  row: 1 | 2,
  viewportWidth: number,
): PlaybackElement[] {
  return ALL_ELEMENTS.filter((el) => elementRow(el, viewportWidth) === row);
}

/** CSS class string for the container. */
export function playbackContainerClass(viewportWidth: number): string {
  return isCompactLayout(viewportWidth)
    ? "playback-controls playback-compact"
    : "playback-controls";
}
