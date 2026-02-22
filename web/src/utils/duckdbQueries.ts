/**
 * DuckDB query functions for time-series charts and sky plot.
 *
 * Uses a generic connection interface so queries can be tested with
 * @duckdb/node-api (vitest) and used in browser with @duckdb/duckdb-wasm.
 */

/** Minimal query interface compatible with both duckdb-wasm and test adapter. */
export interface DuckDBConn {
  query(sql: string): Promise<{ toArray(): unknown[] }>;
}

// ────────────────────────────────────────────
// Time-series chart queries
// ────────────────────────────────────────────

export interface Cn0TimeSeriesRow {
  time_ms: number;
  cn0_mean_all: number;
  cn0_mean_used: number | null;
}

export async function queryCn0TimeSeries(
  conn: DuckDBConn,
): Promise<Cn0TimeSeriesRow[]> {
  const result = await conn.query(`
    SELECT time_ms, cn0_mean_all, cn0_mean_used
    FROM status_epoch
    ORDER BY time_ms
  `);
  return resultToRows(result);
}

export interface SatCountTimeSeriesRow {
  time_ms: number;
  num_visible: number;
  num_used: number;
}

export async function querySatCountTimeSeries(
  conn: DuckDBConn,
): Promise<SatCountTimeSeriesRow[]> {
  const result = await conn.query(`
    SELECT time_ms, num_visible, num_used
    FROM status_epoch
    ORDER BY time_ms
  `);
  return resultToRows(result);
}

export interface AccuracyTimeSeriesRow {
  time_ms: number;
  accuracy_m: number | null;
  vertical_accuracy_m: number | null;
}

export async function queryAccuracyTimeSeries(
  conn: DuckDBConn,
): Promise<AccuracyTimeSeriesRow[]> {
  const result = await conn.query(`
    SELECT time_ms, accuracy_m, vertical_accuracy_m
    FROM fix_epoch
    ORDER BY time_ms
  `);
  return resultToRows(result);
}

export interface SpeedTimeSeriesRow {
  time_ms: number;
  speed_mps: number | null;
}

export async function querySpeedTimeSeries(
  conn: DuckDBConn,
): Promise<SpeedTimeSeriesRow[]> {
  const result = await conn.query(`
    SELECT time_ms, speed_mps
    FROM fix_epoch
    ORDER BY time_ms
  `);
  return resultToRows(result);
}

// ────────────────────────────────────────────
// Sky plot query
// ────────────────────────────────────────────

export interface SkyPlotSatellite {
  constellation: number;
  svid: number;
  azimuth_deg: number;
  elevation_deg: number;
  cn0_dbhz: number;
  used_in_fix: boolean;
}

/**
 * Query satellites visible at a given time (±windowMs/2).
 * Deduplicates by (constellation, svid), keeping highest CN0.
 */
export async function querySkyPlotSatellites(
  conn: DuckDBConn,
  timeMs: number,
  windowMs: number = 1000,
): Promise<SkyPlotSatellite[]> {
  const halfWindow = Math.floor(windowMs / 2);
  const result = await conn.query(`
    WITH ranked AS (
      SELECT
        constellation, svid, azimuth_deg, elevation_deg, cn0_dbhz, used_in_fix,
        ROW_NUMBER() OVER (
          PARTITION BY constellation, svid
          ORDER BY cn0_dbhz DESC
        ) AS rn
      FROM status
      WHERE unix_time_ms BETWEEN ${timeMs - halfWindow} AND ${timeMs + halfWindow}
    )
    SELECT constellation, svid, azimuth_deg, elevation_deg, cn0_dbhz, used_in_fix
    FROM ranked
    WHERE rn = 1
  `);
  return resultToRows(result);
}

// ────────────────────────────────────────────
// Fix quality query
// ────────────────────────────────────────────

export interface FixRow {
  provider: string;
  latitude_deg: number;
  longitude_deg: number;
  altitude_m: number | null;
  unix_time_ms: number;
  accuracy_m: number | null;
  quality: string;
}

export async function queryPrimaryFixes(
  conn: DuckDBConn,
): Promise<FixRow[]> {
  const result = await conn.query(`
    SELECT provider, latitude_deg, longitude_deg, altitude_m,
           unix_time_ms, accuracy_m, quality
    FROM fix
    WHERE quality = 'Primary'
    ORDER BY unix_time_ms
  `);
  return resultToRows(result);
}

// ────────────────────────────────────────────
// Helper: convert DuckDB Arrow result to JS rows
// ────────────────────────────────────────────

function resultToRows<T>(result: { toArray(): unknown[] }): T[] {
  return result.toArray().map((row: unknown) => {
    // DuckDB returns StructRow-like objects; convert to plain objects
    if (row && typeof row === "object" && "toJSON" in row) {
      return (row as { toJSON(): T }).toJSON();
    }
    return row as T;
  });
}
