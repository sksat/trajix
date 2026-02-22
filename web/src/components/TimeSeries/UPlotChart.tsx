/**
 * Generic uPlot React wrapper component.
 *
 * Manages uPlot lifecycle (create/destroy) and provides cursor
 * synchronization across multiple chart instances.
 */
import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

export interface UPlotChartProps {
  /** Chart title. */
  title: string;
  /** uPlot aligned data: [timestamps[], series1[], series2[], ...]. */
  data: uPlot.AlignedData;
  /** Series configuration (colors, labels, etc.). First entry is x-axis. */
  series: uPlot.Series[];
  /** Chart width in pixels. */
  width: number;
  /** Chart height in pixels. */
  height: number;
  /** Shared sync key for cursor synchronization across charts. */
  syncKey: string;
  /** External cursor position (Unix seconds) driven by animation time. */
  cursorTimeSec?: number;
  /** Callback when user moves cursor on the chart. Returns Unix ms. */
  onCursorMove?: (timeMs: number) => void;
  /** Y-axis scale type. */
  scaleType?: "linear" | "log";
}

export function UPlotChart({
  title,
  data,
  series,
  width,
  height,
  syncKey,
  cursorTimeSec,
  onCursorMove,
  scaleType = "linear",
}: UPlotChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const uplotRef = useRef<uPlot | null>(null);

  // Create/destroy uPlot instance
  useEffect(() => {
    if (!containerRef.current || data[0].length === 0) return;

    const opts: uPlot.Options = {
      title,
      width,
      height,
      cursor: {
        sync: { key: syncKey },
      },
      series,
      scales: {
        y: scaleType === "log" ? { distr: 3 } : {},
      },
      hooks: {
        setCursor: [
          (u) => {
            if (!onCursorMove) return;
            const idx = u.cursor.idx;
            if (idx != null && data[0][idx] != null) {
              onCursorMove(data[0][idx] * 1000); // sec → ms
            }
          },
        ],
      },
    };

    const u = new uPlot(opts, data, containerRef.current);
    uplotRef.current = u;

    return () => {
      u.destroy();
      uplotRef.current = null;
    };
    // Only recreate when data identity changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, width, height]);

  // Drive cursor from external time (animation sync)
  useEffect(() => {
    const u = uplotRef.current;
    if (!u || cursorTimeSec == null || data[0].length === 0) return;

    // Find the closest data index for the given time
    const times = data[0]!;
    let lo = 0;
    let hi = times.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if ((times[mid] ?? 0) < cursorTimeSec) lo = mid + 1;
      else hi = mid;
    }

    const timeVal = times[lo];
    if (timeVal == null) return;
    const left = u.valToPos(timeVal, "x");
    if (Number.isFinite(left)) {
      u.setCursor({ left, top: -1 }, false);
    }
  }, [cursorTimeSec, data]);

  return <div ref={containerRef} className="uplot-chart" />;
}
