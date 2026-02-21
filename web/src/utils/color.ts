import { Color } from "cesium";

/**
 * Map horizontal accuracy (meters) to a color.
 * Uses a logarithmic scale:
 *   < 5m  → green
 *   ~10m  → yellow
 *   > 50m → red
 */
export function accuracyToColor(accuracyM: number | null): Color {
  if (accuracyM == null || accuracyM <= 0) return Color.GRAY;

  // Log scale: 5m → 0, 50m → 1
  const t = Math.log10(accuracyM / 5) / Math.log10(10); // log10(50/5) = 1
  const clamped = Math.max(0, Math.min(1, t));

  // Green → Yellow → Red
  if (clamped < 0.5) {
    // green → yellow
    const s = clamped * 2;
    return new Color(s, 1.0, 0.0, 1.0);
  } else {
    // yellow → red
    const s = (clamped - 0.5) * 2;
    return new Color(1.0, 1.0 - s, 0.0, 1.0);
  }
}
