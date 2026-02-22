/**
 * Hook that polls the Cesium clock and provides the current animation
 * time as a reactive Unix timestamp (ms).
 *
 * Used to synchronize time-series charts and sky plot with the 3D map.
 */
import { useCallback, useEffect, useState } from "react";
import * as Cesium from "cesium";

export interface AnimationTime {
  /** Current animation time as Unix timestamp (ms). 0 if not available. */
  currentTimeMs: number;
  /** Start of the animation range (Unix ms). */
  startTimeMs: number;
  /** End of the animation range (Unix ms). */
  endTimeMs: number;
  /** Seek the animation to a specific time (Unix ms). */
  seekTo: (timeMs: number) => void;
}

/** Convert a Cesium JulianDate to Unix milliseconds. */
export function julianToUnixMs(jd: Cesium.JulianDate): number {
  return Cesium.JulianDate.toDate(jd).getTime();
}

export function useAnimationTime(
  viewer: Cesium.Viewer | null,
): AnimationTime {
  const [currentTimeMs, setCurrentTimeMs] = useState(0);
  const [startTimeMs, setStartTimeMs] = useState(0);
  const [endTimeMs, setEndTimeMs] = useState(0);

  // Compute time range once viewer is available
  useEffect(() => {
    if (!viewer) return;
    const clock = viewer.clock;
    setStartTimeMs(julianToUnixMs(clock.startTime));
    setEndTimeMs(julianToUnixMs(clock.stopTime));
  }, [viewer]);

  // Poll Cesium clock at ~10fps for current time
  useEffect(() => {
    if (!viewer) return;

    let raf = 0;
    let lastUpdate = 0;

    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);

      // Throttle to ~10fps
      if (now - lastUpdate < 100) return;
      lastUpdate = now;

      const jd = viewer.clock.currentTime;
      const ms = julianToUnixMs(jd);
      setCurrentTimeMs(ms);
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [viewer]);

  const seekTo = useCallback(
    (timeMs: number) => {
      if (!viewer) return;
      viewer.clock.currentTime = Cesium.JulianDate.fromDate(
        new Date(timeMs),
      );
    },
    [viewer],
  );

  return { currentTimeMs, startTimeMs, endTimeMs, seekTo };
}
