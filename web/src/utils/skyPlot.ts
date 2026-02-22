/**
 * Sky plot utility functions: polar projection, CN0 color, deduplication.
 */

/**
 * Project a satellite's (azimuth, elevation) to SVG (x, y) coordinates
 * on a polar chart.
 *
 * - Azimuth: 0=North, 90=East, 180=South, 270=West (clockwise from North)
 * - Elevation: 0=horizon (outer edge), 90=zenith (center)
 * - SVG origin: top-left; center of circle is at (radius, radius)
 */
export function skyPlotProject(
  azimuthDeg: number,
  elevationDeg: number,
  radius: number,
): { x: number; y: number } {
  // Radial distance: zenith (90°) at center (r=0), horizon (0°) at edge (r=radius)
  const r = radius * (1 - elevationDeg / 90);

  // Convert azimuth to math angle:
  // azimuth 0°=North(up), 90°=East(right)
  // SVG: 0 rad = right, π/2 = down
  // math angle = -(azimuth - 90°) in radians
  // = (90 - azimuth) in degrees
  const theta = ((90 - azimuthDeg) * Math.PI) / 180;

  return {
    x: radius + r * Math.cos(theta),
    y: radius - r * Math.sin(theta),
  };
}

/**
 * Map CN0 (dB-Hz) to a color for sky plot visualization.
 */
export function cn0ToColor(cn0: number): string {
  if (cn0 >= 35) return "#4caf50"; // Strong (green)
  if (cn0 >= 25) return "#ffeb3b"; // Medium (yellow)
  if (cn0 >= 15) return "#ff9800"; // Weak (orange)
  return "#f44336"; // Very weak (red)
}

/** Constellation prefix for satellite labels. */
const CONSTELLATION_LABELS: Record<number, string> = {
  1: "G", // GPS
  3: "R", // GLONASS
  4: "J", // QZSS
  5: "C", // BeiDou
  6: "E", // Galileo
  7: "S", // SBAS
  8: "I", // IRNSS
};

export function constellationLabel(constellation: number): string {
  return CONSTELLATION_LABELS[constellation] ?? "?";
}

/** Constellation color for chart legend / markers. */
const CONSTELLATION_COLORS: Record<number, string> = {
  1: "#4fc3f7", // GPS - light blue
  3: "#ef5350", // GLONASS - red
  4: "#ab47bc", // QZSS - purple
  5: "#ff7043", // BeiDou - deep orange
  6: "#66bb6a", // Galileo - green
  7: "#78909c", // SBAS - blue gray
  8: "#8d6e63", // IRNSS - brown
};

export function constellationColor(constellation: number): string {
  return CONSTELLATION_COLORS[constellation] ?? "#9e9e9e";
}

export interface SkyPlotSatellite {
  constellation: number;
  svid: number;
  azimuth_deg: number;
  elevation_deg: number;
  cn0_dbhz: number;
  used_in_fix: boolean;
}

/**
 * Deduplicate satellites by (constellation, svid), keeping the one
 * with the highest CN0. This handles the case where a satellite
 * appears on multiple frequencies in the same epoch.
 */
export function deduplicateSatellites(
  sats: SkyPlotSatellite[],
): SkyPlotSatellite[] {
  const map = new Map<string, SkyPlotSatellite>();
  for (const s of sats) {
    const key = `${s.constellation}-${s.svid}`;
    const existing = map.get(key);
    if (!existing || s.cn0_dbhz > existing.cn0_dbhz) {
      map.set(key, s);
    }
  }
  return [...map.values()];
}
