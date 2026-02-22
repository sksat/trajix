/**
 * Time-series chart panel with 4 synchronized charts.
 *
 * Displays CN0, satellite count, accuracy, and speed over time.
 * All charts share a cursor sync key for linked crosshair.
 */
import { useMemo, useState, useCallback, useRef, useEffect } from "react";
import type { StatusEpochJs, FixEpochJs } from "../../types/gnss";
import { UPlotChart } from "./UPlotChart";
import {
  statusEpochsToCn0Data,
  statusEpochsToSatCountData,
  fixEpochsToAccuracyData,
  fixEpochsToSpeedData,
} from "./dataConvert";
import "./TimeSeriesPanel.css";

const SYNC_KEY = "trajix-ts";
const CHART_HEIGHT = 150;

interface TimeSeriesPanelProps {
  statusEpochs: StatusEpochJs[];
  fixEpochs: FixEpochJs[];
  currentTimeMs: number;
  onSeek: (timeMs: number) => void;
}

export function TimeSeriesPanel({
  statusEpochs,
  fixEpochs,
  currentTimeMs,
  onSeek,
}: TimeSeriesPanelProps) {
  const [collapsed, setCollapsed] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const [chartWidth, setChartWidth] = useState(400);

  // Observe container width for responsive charts
  useEffect(() => {
    if (!containerRef.current) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        // Each chart gets 1/4 of the width (minus padding)
        const totalWidth = entry.contentRect.width;
        setChartWidth(Math.max(200, Math.floor(totalWidth / 4) - 8));
      }
    });
    ro.observe(containerRef.current);
    return () => ro.disconnect();
  }, []);

  // Memoize data conversions
  const cn0Data = useMemo(
    () => statusEpochsToCn0Data(statusEpochs),
    [statusEpochs],
  );
  const satData = useMemo(
    () => statusEpochsToSatCountData(statusEpochs),
    [statusEpochs],
  );
  const accData = useMemo(
    () => fixEpochsToAccuracyData(fixEpochs),
    [fixEpochs],
  );
  const spdData = useMemo(
    () => fixEpochsToSpeedData(fixEpochs),
    [fixEpochs],
  );

  const cursorTimeSec = currentTimeMs > 0 ? currentTimeMs / 1000 : undefined;

  const handleCursorMove = useCallback(
    (timeMs: number) => {
      onSeek(timeMs);
    },
    [onSeek],
  );

  return (
    <div className={`chart-panel ${collapsed ? "collapsed" : ""}`}>
      <button
        className="chart-panel-toggle"
        onClick={() => setCollapsed(!collapsed)}
      >
        {collapsed ? "Show Charts" : "Hide Charts"}
      </button>
      {!collapsed && (
        <div className="chart-panel-content" ref={containerRef}>
          <UPlotChart
            title="CN0 (dB-Hz)"
            data={cn0Data}
            series={[
              {},
              { label: "All", stroke: "#78909c", width: 1 },
              { label: "Used", stroke: "#42a5f5", width: 1.5 },
            ]}
            width={chartWidth}
            height={CHART_HEIGHT}
            syncKey={SYNC_KEY}
            cursorTimeSec={cursorTimeSec}
            onCursorMove={handleCursorMove}
          />
          <UPlotChart
            title="Satellites"
            data={satData}
            series={[
              {},
              { label: "Visible", stroke: "#78909c", width: 1 },
              { label: "Used", stroke: "#66bb6a", width: 1.5 },
            ]}
            width={chartWidth}
            height={CHART_HEIGHT}
            syncKey={SYNC_KEY}
            cursorTimeSec={cursorTimeSec}
            onCursorMove={handleCursorMove}
          />
          <UPlotChart
            title="Accuracy (m)"
            data={accData}
            series={[
              {},
              { label: "Horizontal", stroke: "#42a5f5", width: 1.5 },
              { label: "Vertical", stroke: "#ff9800", width: 1 },
            ]}
            width={chartWidth}
            height={CHART_HEIGHT}
            syncKey={SYNC_KEY}
            cursorTimeSec={cursorTimeSec}
            onCursorMove={handleCursorMove}
            scaleType="log"
          />
          <UPlotChart
            title="Speed (m/s)"
            data={spdData}
            series={[
              {},
              { label: "Speed", stroke: "#ab47bc", width: 1.5 },
            ]}
            width={chartWidth}
            height={CHART_HEIGHT}
            syncKey={SYNC_KEY}
            cursorTimeSec={cursorTimeSec}
            onCursorMove={handleCursorMove}
          />
        </div>
      )}
    </div>
  );
}
