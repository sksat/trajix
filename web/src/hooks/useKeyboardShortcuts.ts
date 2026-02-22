import { useEffect } from "react";
import type * as Cesium from "cesium";

interface KeyboardShortcutOptions {
  viewer: Cesium.Viewer | null;
  togglePlay: () => void;
  toggleFollow: () => void;
  seekDelta: (deltaMs: number) => void;
}

/**
 * Global keyboard shortcuts for playback control.
 *
 * - Space: play / pause
 * - ArrowLeft / ArrowRight: seek ±10s (×speed multiplier)
 * - F: toggle follow mode
 *
 * Shortcuts are suppressed when focus is inside an input, select, or textarea.
 */
export function useKeyboardShortcuts({
  viewer,
  togglePlay,
  toggleFollow,
  seekDelta,
}: KeyboardShortcutOptions) {
  useEffect(() => {
    if (!viewer) return;

    const onKeyDown = (e: KeyboardEvent) => {
      // Ignore when typing in form elements
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;

      // Ignore when Cesium canvas has focus (arrow keys used for camera)
      if ((e.target as HTMLElement).tagName === "CANVAS") return;

      switch (e.code) {
        case "Space":
          e.preventDefault();
          togglePlay();
          break;
        case "ArrowLeft":
          e.preventDefault();
          seekDelta(-10_000 * viewer.clock.multiplier);
          break;
        case "ArrowRight":
          e.preventDefault();
          seekDelta(10_000 * viewer.clock.multiplier);
          break;
        case "KeyF":
          e.preventDefault();
          toggleFollow();
          break;
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [viewer, togglePlay, toggleFollow, seekDelta]);
}
