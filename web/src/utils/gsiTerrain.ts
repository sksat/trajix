import * as Cesium from "cesium";

/**
 * GSI DEM PNG tile encoding:
 *   raw = R * 65536 + G * 256 + B
 *   (128, 0, 0) = no data
 *   raw < 2^23  → elevation = raw * 0.01 (m)
 *   raw >= 2^23 → elevation = (raw - 2^24) * 0.01 (m, negative)
 */
const NO_DATA_R = 128;
const TILE_SIZE = 256;
const TOTAL_PIXELS = TILE_SIZE * TILE_SIZE;

function decodeGsiElevation(
  r: number,
  g: number,
  b: number,
): number {
  if (r === NO_DATA_R && g === 0 && b === 0) return 0;
  const raw = r * 65536 + g * 256 + b;
  return raw < 8388608 ? raw * 0.01 : (raw - 16777216) * 0.01;
}

// Japan approximate bounding box (Web Mercator tile coords check)
const JAPAN_BOUNDS = {
  west: 122.0 * (Math.PI / 180),
  east: 154.0 * (Math.PI / 180),
  south: 24.0 * (Math.PI / 180),
  north: 46.0 * (Math.PI / 180),
};

/**
 * Create a CesiumJS terrain provider using GSI (国土地理院) DEM PNG tiles.
 * 10m mesh resolution, zoom 1–14, Japan coverage only.
 * No API key required.
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

      const url = `https://cyberjapandata.gsi.go.jp/xyz/dem_png/${level}/${x}/${y}.png`;

      try {
        const res = await fetch(url);
        if (!res.ok) return new Float32Array(TOTAL_PIXELS);

        const blob = await res.blob();
        const bmp = await createImageBitmap(blob);

        const canvas = new OffscreenCanvas(TILE_SIZE, TILE_SIZE);
        const ctx = canvas.getContext("2d")!;
        ctx.drawImage(bmp, 0, 0);
        const { data: pixels } = ctx.getImageData(
          0,
          0,
          TILE_SIZE,
          TILE_SIZE,
        );

        const heights = new Float32Array(TOTAL_PIXELS);
        for (let i = 0; i < TOTAL_PIXELS; i++) {
          heights[i] = decodeGsiElevation(
            pixels[i * 4]!,
            pixels[i * 4 + 1]!,
            pixels[i * 4 + 2]!,
          );
        }
        return heights;
      } catch {
        return new Float32Array(TOTAL_PIXELS);
      }
    },
  });
}
