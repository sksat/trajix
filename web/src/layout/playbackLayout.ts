/**
 * Playback controls layout logic.
 *
 * On mobile (portrait or <768px), the controls split into two rows:
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
export function isCompactLayout(
  viewportWidth: number,
  viewportHeight?: number,
): boolean {
  return isMobile(viewportWidth, viewportHeight);
}

/** Which row an element belongs to (1 = top, 2 = bottom). */
export function elementRow(
  element: PlaybackElement,
  viewportWidth: number,
  viewportHeight?: number,
): 1 | 2 {
  if (!isMobile(viewportWidth, viewportHeight)) return 1;
  return ROW2_COMPACT.has(element) ? 2 : 1;
}

/** Ordered elements for a given row. */
export function rowElements(
  row: 1 | 2,
  viewportWidth: number,
  viewportHeight?: number,
): PlaybackElement[] {
  return ALL_ELEMENTS.filter(
    (el) => elementRow(el, viewportWidth, viewportHeight) === row,
  );
}

/** CSS class string for the container. */
export function playbackContainerClass(
  viewportWidth: number,
  viewportHeight?: number,
): string {
  return isCompactLayout(viewportWidth, viewportHeight)
    ? "playback-controls playback-compact"
    : "playback-controls";
}
