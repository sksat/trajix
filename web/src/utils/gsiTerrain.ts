import * as Cesium from "cesium";
import {
  decodePngValue,
  decodeGeoidValue,
  addGeoidCorrection,
  TILE_SIZE,
  TOTAL_PIXELS,
  GEOID_MAX_ZOOM,
} from "./geoid";

// Japan approximate bounding box (Web Mercator tile coords check)
const JAPAN_BOUNDS = {
  west: 122.0 * (Math.PI / 180),
  east: 154.0 * (Math.PI / 180),
  south: 24.0 * (Math.PI / 180),
  north: 46.0 * (Math.PI / 180),
};

// ────────────────────────────────────────────
// PNG tile decoding (shared for DEM + geoid)
// ────────────────────────────────────────────

type PixelDecoder = (r: number, g: number, b: number) => number;

/** Decode a PNG tile from a URL into a Float32Array using the given decoder. */
async function decodePngTile(
  url: string,
  decode: PixelDecoder,
): Promise<Float32Array | null> {
  const res = await fetch(url);
  if (!res.ok) return null;

  const blob = await res.blob();
  const bmp = await createImageBitmap(blob);

  const canvas = new OffscreenCanvas(TILE_SIZE, TILE_SIZE);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bmp, 0, 0);
  const { data: pixels } = ctx.getImageData(0, 0, TILE_SIZE, TILE_SIZE);

  const values = new Float32Array(TOTAL_PIXELS);
  for (let i = 0; i < TOTAL_PIXELS; i++) {
    values[i] = decode(
      pixels[i * 4]!,
      pixels[i * 4 + 1]!,
      pixels[i * 4 + 2]!,
    );
  }
  return values;
}

// ────────────────────────────────────────────
// Geoid tile cache
// ────────────────────────────────────────────

const geoidCache = new Map<string, Float32Array>();
const geoidInFlight = new Map<string, Promise<Float32Array>>();

async function fetchGeoidTile(
  x: number,
  y: number,
  level: number,
): Promise<Float32Array> {
  const key = `${level}/${x}/${y}`;

  const cached = geoidCache.get(key);
  if (cached) return cached;

  let promise = geoidInFlight.get(key);
  if (!promise) {
    promise = (async () => {
      // GSJ geoid tiles use {z}/{y}/{x} order (unlike GSI DEM which uses {z}/{x}/{y})
      const url = `https://tiles.gsj.jp/tiles/elev/gsigeoid/${level}/${y}/${x}.png`;
      try {
        const tile = await decodePngTile(url, decodeGeoidValue);
        const result = tile ?? new Float32Array(TOTAL_PIXELS);
        geoidCache.set(key, result);
        return result;
      } catch {
        const empty = new Float32Array(TOTAL_PIXELS);
        geoidCache.set(key, empty);
        return empty;
      } finally {
        geoidInFlight.delete(key);
      }
    })();
    geoidInFlight.set(key, promise);
  }

  return promise;
}

// ────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────

/**
 * Create a CesiumJS terrain provider using GSI (国土地理院) DEM PNG tiles
 * with GSIGEO2011 geoid correction.
 *
 * GSI DEM tiles provide orthometric height (標高, above Tokyo Bay MSL).
 * CesiumJS expects ellipsoidal height (above WGS84 ellipsoid).
 * We add geoid undulation N to convert: h = H + N.
 *
 * 10m mesh resolution, zoom 1–14, Japan coverage only. No API key required.
 */
export function createGsiTerrainProvider(): Cesium.CustomHeightmapTerrainProvider {
  const tilingScheme = new Cesium.WebMercatorTilingScheme();

  return new Cesium.CustomHeightmapTerrainProvider({
    width: TILE_SIZE,
    height: TILE_SIZE,
    tilingScheme,
    credit: new Cesium.Credit(
      '<a href="https://maps.gsi.go.jp/development/ichiran.html">国土地理院</a>',
    ),
    callback: async (
      x: number,
      y: number,
      level: number,
    ): Promise<Float32Array> => {
      // GSI DEM tiles are available at zoom 1–14
      if (level > 14) {
        return new Float32Array(TOTAL_PIXELS);
      }

      // Skip tiles outside Japan to avoid 404 noise
      const rect = tilingScheme.tileXYToRectangle(x, y, level);
      if (
        rect.east < JAPAN_BOUNDS.west ||
        rect.west > JAPAN_BOUNDS.east ||
        rect.north < JAPAN_BOUNDS.south ||
        rect.south > JAPAN_BOUNDS.north
      ) {
        return new Float32Array(TOTAL_PIXELS);
      }

      // Fetch DEM tile (orthometric heights)
      const demUrl = `https://cyberjapandata.gsi.go.jp/xyz/dem_png/${level}/${x}/${y}.png`;
      let demHeights: Float32Array;
      try {
        const tile = await decodePngTile(demUrl, decodePngValue);
        if (!tile) return new Float32Array(TOTAL_PIXELS);
        demHeights = tile;
      } catch {
        return new Float32Array(TOTAL_PIXELS);
      }

      // Fetch geoid tile and apply correction (orthometric → ellipsoidal)
      const geoidLevel = Math.min(level, GEOID_MAX_ZOOM);
      const shift = Math.max(0, level - GEOID_MAX_ZOOM);
      const geoidX = x >> shift;
      const geoidY = y >> shift;

      try {
        const geoidTile = await fetchGeoidTile(geoidX, geoidY, geoidLevel);
        return addGeoidCorrection(
          demHeights,
          geoidTile,
          x,
          y,
          level,
          geoidLevel,
        );
      } catch {
        // Geoid fetch failed: return DEM without correction as fallback
        return demHeights;
      }
    },
  });
}
