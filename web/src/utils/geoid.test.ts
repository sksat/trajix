import { describe, it, expect } from "vitest";
import {
  decodePngValue,
  decodeGeoidValue,
  demToGeoidTile,
  addGeoidCorrection,
  TILE_SIZE,
  TOTAL_PIXELS,
  GEOID_MAX_ZOOM,
} from "./geoid";

// ────────────────────────────────────────────
// decodePngValue
// ────────────────────────────────────────────

describe("decodePngValue", () => {
  it("decodes no-data sentinel (128, 0, 0) as 0", () => {
    expect(decodePngValue(128, 0, 0)).toBe(0);
  });

  it("decodes zero elevation (0, 0, 0)", () => {
    // raw = 0 → 0 * 0.01 = 0.0
    expect(decodePngValue(0, 0, 0)).toBe(0);
  });

  it("decodes positive elevation: 100.00 m", () => {
    // 100.00 m → raw = 10000
    // 10000 = 0 * 65536 + 39 * 256 + 16
    const raw = 10000;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(r).toBe(0);
    expect(g).toBe(39);
    expect(b).toBe(16);
    expect(decodePngValue(r, g, b)).toBeCloseTo(100.0, 2);
  });

  it("decodes positive elevation: 40.00 m (typical geoid value)", () => {
    // 40.00 m → raw = 4000
    // 4000 = 0 * 65536 + 15 * 256 + 160
    const raw = 4000;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodePngValue(r, g, b)).toBeCloseTo(40.0, 2);
  });

  it("decodes positive elevation: 877.00 m (Mt. Tsukuba summit)", () => {
    const raw = 87700;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodePngValue(r, g, b)).toBeCloseTo(877.0, 2);
  });

  it("decodes negative elevation: -5.00 m", () => {
    // -5.00 m → raw = 16777216 + (-500) = 16776716
    const raw = 16776716;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodePngValue(r, g, b)).toBeCloseTo(-5.0, 2);
  });

  it("decodes boundary: raw = 2^23 - 1 (max positive)", () => {
    // raw = 8388607 → 83886.07 m
    const raw = 8388607;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodePngValue(r, g, b)).toBeCloseTo(83886.07, 2);
  });

  it("raw = 2^23 collides with no-data sentinel (128, 0, 0)", () => {
    // raw = 8388608 → R=128, G=0, B=0 — same as no-data sentinel
    // This is a known ambiguity in the encoding; returns 0
    expect(decodePngValue(128, 0, 0)).toBe(0);
  });

  it("decodes first non-sentinel negative: raw = 2^23 + 1", () => {
    // raw = 8388609 → (8388609 - 16777216) * 0.01 = -83886.07
    const raw = 8388609;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(r).toBe(128);
    expect(g).toBe(0);
    expect(b).toBe(1);
    expect(decodePngValue(r, g, b)).toBeCloseTo(-83886.07, 2);
  });

  it("decodes small positive: 0.01 m", () => {
    // raw = 1
    expect(decodePngValue(0, 0, 1)).toBeCloseTo(0.01, 4);
  });

  it("decodes small negative: -0.01 m", () => {
    // raw = 16777216 - 1 = 16777215 = (255, 255, 255)
    expect(decodePngValue(255, 255, 255)).toBeCloseTo(-0.01, 4);
  });
});

// ────────────────────────────────────────────
// decodeGeoidValue (scale = 0.0001)
// ────────────────────────────────────────────

describe("decodeGeoidValue", () => {
  it("decodes no-data sentinel (128, 0, 0) as 0", () => {
    expect(decodeGeoidValue(128, 0, 0)).toBe(0);
  });

  it("decodes Tsukuba-area geoid: ~40 m", () => {
    // Verified from actual GSJ tile at zoom 8, tile (227, 99), pixel (0, 0):
    // R=6, G=32, B=249, raw=401657
    // scale 0.0001: 401657 * 0.0001 = 40.1657 m ✓
    expect(decodeGeoidValue(6, 32, 249)).toBeCloseTo(40.1657, 3);
  });

  it("decodes Tsukuba center geoid: ~43.7 m", () => {
    // Actual GSJ tile pixel (128, 128): R=6, G=172, B=203, raw=437451
    expect(decodeGeoidValue(6, 172, 203)).toBeCloseTo(43.7451, 3);
  });

  it("decodes zero (0, 0, 0)", () => {
    expect(decodeGeoidValue(0, 0, 0)).toBe(0);
  });

  it("scale factor is 0.0001, not 0.01", () => {
    // raw = 400000 → scale 0.01 = 4000m (WRONG), scale 0.0001 = 40m (CORRECT)
    const raw = 400000;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    const result = decodeGeoidValue(r, g, b);
    expect(result).toBeCloseTo(40.0, 2);
    // Must NOT be ~4000
    expect(result).toBeLessThan(100);
  });

  it("decodes small geoid: 30 m (southern Japan)", () => {
    // 30.0 m → raw = 300000
    const raw = 300000;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodeGeoidValue(r, g, b)).toBeCloseTo(30.0, 2);
  });

  it("decodes large geoid: 45 m (northern Honshu)", () => {
    // 45.0 m → raw = 450000
    const raw = 450000;
    const r = Math.floor(raw / 65536);
    const g = Math.floor((raw % 65536) / 256);
    const b = raw % 256;
    expect(decodeGeoidValue(r, g, b)).toBeCloseTo(45.0, 2);
  });
});

// ────────────────────────────────────────────
// demToGeoidTile
// ────────────────────────────────────────────

describe("demToGeoidTile", () => {
  it("returns same coordinates for zoom <= GEOID_MAX_ZOOM", () => {
    expect(demToGeoidTile(5, 3, 5)).toEqual({
      geoidX: 5,
      geoidY: 3,
      geoidLevel: 5,
    });
  });

  it("returns same coordinates at exactly GEOID_MAX_ZOOM", () => {
    expect(demToGeoidTile(100, 200, GEOID_MAX_ZOOM)).toEqual({
      geoidX: 100,
      geoidY: 200,
      geoidLevel: GEOID_MAX_ZOOM,
    });
  });

  it("scales down for zoom 9 (1 level above max)", () => {
    // zoom 9 tile (4, 6) → zoom 8 tile (2, 3)
    expect(demToGeoidTile(4, 6, 9)).toEqual({
      geoidX: 2,
      geoidY: 3,
      geoidLevel: 8,
    });
  });

  it("scales down for zoom 14 (6 levels above max)", () => {
    // zoom 14 tile (14560, 6456) → shift by 6 bits
    // 14560 >> 6 = 227, 6456 >> 6 = 100
    expect(demToGeoidTile(14560, 6456, 14)).toEqual({
      geoidX: 227,
      geoidY: 100,
      geoidLevel: 8,
    });
  });

  it("handles zoom 0", () => {
    expect(demToGeoidTile(0, 0, 0)).toEqual({
      geoidX: 0,
      geoidY: 0,
      geoidLevel: 0,
    });
  });

  it("multiple DEM tiles map to same geoid tile", () => {
    // At zoom 10, tiles (0,0), (1,0), (0,1), (1,1) all map to zoom 8 (0,0)
    // shift = 10 - 8 = 2, scale = 4
    // Tiles 0-3 in x and y all map to geoid 0
    for (let dx = 0; dx < 4; dx++) {
      for (let dy = 0; dy < 4; dy++) {
        expect(demToGeoidTile(dx, dy, 10)).toEqual({
          geoidX: 0,
          geoidY: 0,
          geoidLevel: 8,
        });
      }
    }
    // Tile (4, 0) at zoom 10 maps to geoid (1, 0)
    expect(demToGeoidTile(4, 0, 10)).toEqual({
      geoidX: 1,
      geoidY: 0,
      geoidLevel: 8,
    });
  });
});

// ────────────────────────────────────────────
// addGeoidCorrection
// ────────────────────────────────────────────

describe("addGeoidCorrection", () => {
  /** Create a Float32Array filled with a constant value. */
  function filledArray(value: number): Float32Array {
    const arr = new Float32Array(TOTAL_PIXELS);
    arr.fill(value);
    return arr;
  }

  it("adds geoid to DEM at same zoom (1:1 mapping)", () => {
    const dem = filledArray(100); // 100m orthometric
    const geoid = filledArray(40); // 40m geoid undulation
    const result = addGeoidCorrection(dem, geoid, 5, 3, 8, 8);

    // All pixels should be 140m ellipsoidal
    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(140, 2);
    }
  });

  it("handles zero geoid (no correction)", () => {
    const dem = filledArray(250);
    const geoid = filledArray(0);
    const result = addGeoidCorrection(dem, geoid, 0, 0, 5, 5);

    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(250, 2);
    }
  });

  it("handles zero DEM with nonzero geoid", () => {
    const dem = filledArray(0); // sea level or no-data
    const geoid = filledArray(39.5);
    const result = addGeoidCorrection(dem, geoid, 0, 0, 5, 5);

    // Should give geoid height (~sea level in ellipsoidal terms)
    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(39.5, 2);
    }
  });

  it("handles negative DEM with geoid", () => {
    const dem = filledArray(-10); // below sea level
    const geoid = filledArray(40);
    const result = addGeoidCorrection(dem, geoid, 0, 0, 5, 5);

    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(30, 2);
    }
  });

  it("correctly maps DEM pixels to geoid at higher zoom (zoom 9 → 8)", () => {
    // DEM at zoom 9, geoid at zoom 8
    // shift = 1, scale = 2
    // DEM tile (0, 0) at zoom 9 maps to top-left quadrant of geoid tile (0, 0) at zoom 8
    const dem = filledArray(100);
    const geoid = new Float32Array(TOTAL_PIXELS);

    // Fill geoid with spatially varying values:
    // Left half = 35m, right half = 45m
    for (let py = 0; py < TILE_SIZE; py++) {
      for (let px = 0; px < TILE_SIZE; px++) {
        geoid[py * TILE_SIZE + px] = px < TILE_SIZE / 2 ? 35 : 45;
      }
    }

    // DEM tile (0, 0) at zoom 9 is the top-left quadrant of geoid tile (0, 0) at zoom 8
    // localX = 0, localY = 0
    // DEM pixel (px, py) maps to geoid pixel (px >> 1, py >> 1)
    // So DEM pixels 0..127 map to geoid pixels 0..63 (left half of geoid's left half → 35m)
    // DEM pixels 128..255 map to geoid pixels 64..127 (right half of geoid's left half → 35m)
    // Wait, all DEM pixels map to the left half (first 128 columns) of the geoid tile
    // because localX=0, scale=2, so gx = (0*256 + px) >> 1 = px >> 1 = 0..127
    // Geoid left half (0..127) = 35m
    const result = addGeoidCorrection(dem, geoid, 0, 0, 9, 8);

    // All pixels should have geoid value 35 (from left half of geoid)
    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(135, 2);
    }
  });

  it("correctly maps second sub-tile at zoom 9 → 8", () => {
    // DEM tile (1, 0) at zoom 9 maps to top-right quadrant of geoid tile (0, 0) at zoom 8
    const dem = filledArray(100);
    const geoid = new Float32Array(TOTAL_PIXELS);

    // Left half = 35m, right half = 45m
    for (let py = 0; py < TILE_SIZE; py++) {
      for (let px = 0; px < TILE_SIZE; px++) {
        geoid[py * TILE_SIZE + px] = px < TILE_SIZE / 2 ? 35 : 45;
      }
    }

    // DEM tile (1, 0): localX = 1, localY = 0
    // gx = (1*256 + px) >> 1 = (256 + px) >> 1 = 128 + (px >> 1) = 128..255
    // Geoid right half (128..255) = 45m
    const result = addGeoidCorrection(dem, geoid, 1, 0, 9, 8);

    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(145, 2);
    }
  });

  it("maps correctly at zoom 14 → 8 (6 level difference)", () => {
    // shift = 6, scale = 64
    // DEM tile (0, 0) at zoom 14 maps to a tiny portion of geoid tile (0, 0) at zoom 8
    const dem = filledArray(500);
    const geoid = filledArray(39.5);

    const result = addGeoidCorrection(dem, geoid, 0, 0, 14, 8);

    for (let i = 0; i < TOTAL_PIXELS; i++) {
      expect(result[i]).toBeCloseTo(539.5, 2);
    }
  });

  it("maps correctly at zoom 14 with non-zero local offset", () => {
    // DEM tile (64, 64) at zoom 14 → geoid tile (1, 1) at zoom 8
    // localX = 64 % 64 = 0, localY = 64 % 64 = 0
    // But DEM tile (65, 64) → geoid tile (1, 1), localX = 1
    const dem = filledArray(200);

    // Geoid with gradient: value = px (so 0..255 from left to right)
    const geoid = new Float32Array(TOTAL_PIXELS);
    for (let py = 0; py < TILE_SIZE; py++) {
      for (let px = 0; px < TILE_SIZE; px++) {
        geoid[py * TILE_SIZE + px] = px;
      }
    }

    // DEM tile (65, 64) at zoom 14: localX = 1, localY = 0
    // gx = (1 * 256 + px) >> 6 = (256 + px) / 64 floored
    // px=0: gx = 256/64 = 4
    // px=255: gx = (256+255)/64 = 511/64 = 7
    // So all geoid values should be between 4 and 7
    const result = addGeoidCorrection(dem, geoid, 65, 64, 14, 8);

    // Check a few pixels
    expect(result[0]).toBeCloseTo(200 + 4, 2); // px=0: gx=4
    expect(result[255]).toBeCloseTo(200 + 7, 2); // px=255: gx=7
  });

  it("does not modify input arrays", () => {
    const dem = filledArray(100);
    const geoid = filledArray(40);
    const demCopy = new Float32Array(dem);
    const geoidCopy = new Float32Array(geoid);

    addGeoidCorrection(dem, geoid, 0, 0, 8, 8);

    expect(dem).toEqual(demCopy);
    expect(geoid).toEqual(geoidCopy);
  });

  it("pixel-level accuracy: per-pixel geoid variation at same zoom", () => {
    const dem = filledArray(100);
    const geoid = new Float32Array(TOTAL_PIXELS);

    // Set specific pixels with different geoid values
    geoid[0] = 30; // top-left
    geoid[TILE_SIZE - 1] = 35; // top-right
    geoid[(TILE_SIZE - 1) * TILE_SIZE] = 40; // bottom-left
    geoid[TOTAL_PIXELS - 1] = 45; // bottom-right

    const result = addGeoidCorrection(dem, geoid, 0, 0, 5, 5);

    expect(result[0]).toBeCloseTo(130, 2);
    expect(result[TILE_SIZE - 1]).toBeCloseTo(135, 2);
    expect(result[(TILE_SIZE - 1) * TILE_SIZE]).toBeCloseTo(140, 2);
    expect(result[TOTAL_PIXELS - 1]).toBeCloseTo(145, 2);
  });
});
