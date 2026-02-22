/**
 * Sky plot: polar SVG chart showing satellite positions.
 *
 * Uses in-memory binary search on sorted satellite_snapshots array
 * for fast lookups at the current animation time.
 */
import { useMemo, useState } from "react";
import type { SatelliteSnapshotJs } from "../../types/gnss";
import {
  skyPlotProject,
  cn0ToColor,
  constellationLabel,
  constellationColor,
  deduplicateSatellites,
} from "../../utils/skyPlot";
import "./SkyPlot.css";

const RADIUS = 90;
const CENTER = RADIUS + 10; // Padding for labels
const SVG_SIZE = CENTER * 2;
const ELEVATION_RINGS = [0, 15, 30, 45, 60, 75];

interface SkyPlotProps {
  snapshots: SatelliteSnapshotJs[];
  currentTimeMs: number;
}

/**
 * Binary search for the first index where snapshot.time_ms >= target.
 */
function lowerBound(
  snapshots: SatelliteSnapshotJs[],
  target: number,
): number {
  let lo = 0;
  let hi = snapshots.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (snapshots[mid]!.time_ms < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * Get satellites visible at the given time (±500ms window).
 */
function getSatellitesAtTime(
  snapshots: SatelliteSnapshotJs[],
  timeMs: number,
): SatelliteSnapshotJs[] {
  if (snapshots.length === 0 || timeMs <= 0) return [];

  const halfWindow = 500;
  const startIdx = lowerBound(snapshots, timeMs - halfWindow);
  const endIdx = lowerBound(snapshots, timeMs + halfWindow + 1);

  return snapshots.slice(startIdx, endIdx);
}

const LEGEND_ITEMS: { constellation: number; label: string }[] = [
  { constellation: 1, label: "GPS" },
  { constellation: 3, label: "GLO" },
  { constellation: 6, label: "GAL" },
  { constellation: 5, label: "BDS" },
  { constellation: 4, label: "QZS" },
];

export function SkyPlot({ snapshots, currentTimeMs }: SkyPlotProps) {
  const [activeConstellations, setActiveConstellations] = useState<
    ReadonlySet<number>
  >(new Set());

  const satellites = useMemo(() => {
    const raw = getSatellitesAtTime(snapshots, currentTimeMs);
    const deduped = deduplicateSatellites(raw);
    if (activeConstellations.size === 0) return deduped;
    return deduped.filter((s) => activeConstellations.has(s.constellation));
  }, [snapshots, currentTimeMs, activeConstellations]);

  function handleLegendClick(constellation: number) {
    setActiveConstellations((prev) => {
      const next = new Set(prev);
      if (next.has(constellation)) next.delete(constellation);
      else next.add(constellation);
      return next;
    });
  }

  return (
    <div className="sky-plot">
      <svg viewBox={`0 0 ${SVG_SIZE} ${SVG_SIZE}`}>
        {/* Background */}
        <circle
          cx={CENTER}
          cy={CENTER}
          r={RADIUS}
          fill="rgba(15, 15, 26, 0.6)"
          stroke="#2a2a4a"
          strokeWidth={1}
        />

        {/* Elevation rings */}
        {ELEVATION_RINGS.map((el) => {
          const r = RADIUS * (1 - el / 90);
          return (
            <circle
              key={el}
              cx={CENTER}
              cy={CENTER}
              r={r}
              fill="none"
              stroke="#1a1a3a"
              strokeWidth={0.5}
            />
          );
        })}

        {/* Cross lines (N-S, E-W) */}
        <line
          x1={CENTER}
          y1={CENTER - RADIUS}
          x2={CENTER}
          y2={CENTER + RADIUS}
          stroke="#1a1a3a"
          strokeWidth={0.5}
        />
        <line
          x1={CENTER - RADIUS}
          y1={CENTER}
          x2={CENTER + RADIUS}
          y2={CENTER}
          stroke="#1a1a3a"
          strokeWidth={0.5}
        />

        {/* Cardinal direction labels */}
        <text x={CENTER} y={CENTER - RADIUS - 3} className="sky-label">
          N
        </text>
        <text x={CENTER + RADIUS + 5} y={CENTER + 3} className="sky-label">
          E
        </text>
        <text x={CENTER} y={CENTER + RADIUS + 12} className="sky-label">
          S
        </text>
        <text x={CENTER - RADIUS - 10} y={CENTER + 3} className="sky-label">
          W
        </text>

        {/* Satellite markers */}
        {satellites.map((sat) => {
          const { x, y } = skyPlotProject(
            sat.azimuth_deg,
            sat.elevation_deg,
            RADIUS,
          );
          const px = x + (CENTER - RADIUS);
          const py = y + (CENTER - RADIUS);
          const r = sat.used_in_fix ? 5 : 3;
          const color = sat.used_in_fix
            ? constellationColor(sat.constellation)
            : cn0ToColor(sat.cn0_dbhz);

          return (
            <g key={`${sat.constellation}-${sat.svid}`}>
              <circle
                cx={px}
                cy={py}
                r={r}
                fill={color}
                stroke={sat.used_in_fix ? "#fff" : "none"}
                strokeWidth={sat.used_in_fix ? 1 : 0}
                opacity={sat.used_in_fix ? 1 : 0.7}
              />
              <text
                x={px}
                y={py - r - 2}
                className="sat-label"
                fill={color}
              >
                {constellationLabel(sat.constellation)}
                {sat.svid}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="sky-plot-legend">
        {LEGEND_ITEMS.map(({ constellation, label }) => {
          const isActive = activeConstellations.has(constellation);
          const isInactive =
            activeConstellations.size > 0 && !isActive;
          const cls = [
            "sky-legend-item",
            isActive ? "active" : "",
            isInactive ? "inactive" : "",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <span
              key={constellation}
              className={cls}
              onClick={() => handleLegendClick(constellation)}
            >
              <span
                className="legend-dot"
                style={{ background: constellationColor(constellation) }}
              />
              {label}
            </span>
          );
        })}
      </div>
    </div>
  );
}
