/**
 * Geoid correction utilities for converting between orthometric height (標高)
 * and ellipsoidal height (WGS84).
 *
 * Relationship: h_ellipsoidal = H_orthometric + N_geoid
 *
 * - GSI DEM tiles provide orthometric height H (above Tokyo Bay MSL)
 * - Android GNSS getAltitude() provides ellipsoidal height h (above WGS84)
 * - CesiumJS expects ellipsoidal height h
 * - Geoid undulation N ≈ 30–45 m across Japan (GSIGEO2011)
 */

export const TILE_SIZE = 256;
export const TOTAL_PIXELS = TILE_SIZE * TILE_SIZE;

/** Maximum zoom level for GSJ geoid tiles. */
export const GEOID_MAX_ZOOM = 8;

/**
 * Decode a GSI DEM PNG tile pixel to a height value in meters.
 *
 * Encoding (標高タイル format, scale = 0.01):
 *   raw = R * 65536 + G * 256 + B
 *   (128, 0, 0) = no data → returns 0
 *   raw < 2^23  → value = raw * 0.01
 *   raw >= 2^23 → value = (raw - 2^24) * 0.01 (negative)
 */
export function decodePngValue(r: number, g: number, b: number): number {
  if (r === 128 && g === 0 && b === 0) return 0; // no-data sentinel
  const raw = r * 65536 + g * 256 + b;
  return raw < 8388608 ? raw * 0.01 : (raw - 16777216) * 0.01;
}

/**
 * Decode a GSJ geoid PNG tile pixel to a geoid undulation value in meters.
 *
 * Same RGB encoding as DEM but with finer scale factor (0.0001).
 * Geoid undulation in Japan: ~30–45 m (positive).
 */
export function decodeGeoidValue(r: number, g: number, b: number): number {
  if (r === 128 && g === 0 && b === 0) return 0; // no-data sentinel
  const raw = r * 65536 + g * 256 + b;
  return raw < 8388608 ? raw * 0.0001 : (raw - 16777216) * 0.0001;
}

/**
 * Compute the geoid tile coordinates for a given DEM tile.
 *
 * For DEM zoom <= GEOID_MAX_ZOOM: same tile coordinates.
 * For DEM zoom > GEOID_MAX_ZOOM: scale down to zoom 8.
 */
export function demToGeoidTile(
  demX: number,
  demY: number,
  demLevel: number,
): { geoidX: number; geoidY: number; geoidLevel: number } {
  const geoidLevel = Math.min(demLevel, GEOID_MAX_ZOOM);
  const shift = Math.max(0, demLevel - GEOID_MAX_ZOOM);
  return {
    geoidX: demX >> shift,
    geoidY: demY >> shift,
    geoidLevel,
  };
}

/**
 * Add geoid undulation to DEM heights (orthometric → ellipsoidal).
 *
 * For DEM zoom <= geoid zoom: 1:1 pixel mapping.
 * For DEM zoom > geoid zoom: each DEM tile is a sub-region of a
 * geoid tile. DEM pixels are mapped to the corresponding geoid pixels
 * via nearest-neighbor lookup.
 *
 * The geoid varies very slowly (~1 m per 100 km), so nearest-neighbor
 * is sufficient (sub-centimeter error within a single DEM tile).
 *
 * @returns New Float32Array of ellipsoidal heights.
 */
export function addGeoidCorrection(
  demHeights: Float32Array,
  geoidTile: Float32Array,
  demX: number,
  demY: number,
  demLevel: number,
  geoidLevel: number,
): Float32Array {
  const heights = new Float32Array(TOTAL_PIXELS);
  const shift = demLevel - geoidLevel;

  if (shift <= 0) {
    // Same zoom: 1:1 pixel mapping
    for (let i = 0; i < TOTAL_PIXELS; i++) {
      heights[i] = demHeights[i]! + geoidTile[i]!;
    }
    return heights;
  }

  // DEM at higher zoom than geoid: map DEM pixels → geoid pixels
  const scale = 1 << shift;
  const localX = demX & (scale - 1); // demX % scale
  const localY = demY & (scale - 1); // demY % scale

  for (let py = 0; py < TILE_SIZE; py++) {
    const gy = Math.min(
      (localY * TILE_SIZE + py) >> shift,
      TILE_SIZE - 1,
    );
    for (let px = 0; px < TILE_SIZE; px++) {
      const gx = Math.min(
        (localX * TILE_SIZE + px) >> shift,
        TILE_SIZE - 1,
      );
      const idx = py * TILE_SIZE + px;
      heights[idx] = demHeights[idx]! + geoidTile[gy * TILE_SIZE + gx]!;
    }
  }
  return heights;
}
