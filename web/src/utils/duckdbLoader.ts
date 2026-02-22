/**
 * Load ProcessingResult data into DuckDB tables.
 *
 * Uses JSON serialization + DuckDB's read_json_auto for bulk insertion.
 */
import type { AsyncDuckDB, AsyncDuckDBConnection } from "@duckdb/duckdb-wasm";
import type { ProcessingResult } from "../types/gnss";
import { SCHEMA_SQL } from "./duckdbSchema";

/**
 * Create tables and load all data from ProcessingResult into DuckDB.
 */
export async function loadResultIntoDuckDB(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  result: ProcessingResult,
): Promise<void> {
  // Create schema
  await conn.query(SCHEMA_SQL);

  // Load fix records with quality tags
  await loadFixes(db, conn, result);

  // Load satellite snapshots into status table
  await loadSatelliteSnapshots(db, conn, result);

  // Load status epochs
  await loadStatusEpochs(db, conn, result);

  // Load fix epochs
  await loadFixEpochs(db, conn, result);
}

/**
 * Map a FixRecord (from serde-wasm-bindgen) + quality tag into a plain object
 * suitable for JSON serialization into DuckDB.
 *
 * serde-wasm-bindgen returns `undefined` for Option::None, but JSON.stringify
 * omits undefined properties. This causes read_json_auto to see fewer columns
 * than the schema expects. We coerce undefined→null so all 16 keys are always
 * present in the serialized JSON.
 */
export function mapFixRow(f: any, quality: string) {
  return {
    provider: f.provider,
    latitude_deg: f.latitude_deg,
    longitude_deg: f.longitude_deg,
    altitude_m: f.altitude_m ?? null,
    speed_mps: f.speed_mps ?? null,
    accuracy_m: f.accuracy_m ?? null,
    bearing_deg: f.bearing_deg ?? null,
    unix_time_ms: f.unix_time_ms,
    speed_accuracy_mps: f.speed_accuracy_mps ?? null,
    bearing_accuracy_deg: f.bearing_accuracy_deg ?? null,
    elapsed_realtime_ns: f.elapsed_realtime_ns ?? null,
    vertical_accuracy_m: f.vertical_accuracy_m ?? null,
    mock_location: f.mock_location,
    num_used_signals: f.num_used_signals ?? null,
    solution_type: f.solution_type ?? null,
    quality,
  };
}

async function loadFixes(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  result: ProcessingResult,
): Promise<void> {
  if (result.fixes.length === 0) return;

  const rows = result.fixes.map((f, i) => mapFixRow(f, result.fix_qualities[i]));

  await insertJsonArray(db, conn, "fix", rows);
}

async function loadSatelliteSnapshots(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  result: ProcessingResult,
): Promise<void> {
  if (!result.satellite_snapshots || result.satellite_snapshots.length === 0)
    return;

  const rows = result.satellite_snapshots.map((s) => ({
    unix_time_ms: s.time_ms,
    constellation: s.constellation,
    svid: s.svid,
    azimuth_deg: s.azimuth_deg,
    elevation_deg: s.elevation_deg,
    cn0_dbhz: s.cn0_dbhz,
    used_in_fix: s.used_in_fix,
  }));

  await insertJsonArray(db, conn, "status", rows);
}

async function loadStatusEpochs(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  result: ProcessingResult,
): Promise<void> {
  if (result.status_epochs.length === 0) return;

  const rows = result.status_epochs.map((e) => ({
    time_ms: e.time_ms,
    cn0_mean_all: e.cn0_mean_all,
    cn0_mean_used: Number.isNaN(e.cn0_mean_used) ? null : e.cn0_mean_used,
    num_visible: e.num_visible,
    num_used: e.num_used,
  }));

  await insertJsonArray(db, conn, "status_epoch", rows);
}

async function loadFixEpochs(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  result: ProcessingResult,
): Promise<void> {
  if (result.fix_epochs.length === 0) return;

  const rows = result.fix_epochs.map((e) => ({
    time_ms: e.time_ms,
    accuracy_m: e.accuracy_m,
    vertical_accuracy_m: e.vertical_accuracy_m,
    speed_mps: e.speed_mps,
  }));

  await insertJsonArray(db, conn, "fix_epoch", rows);
}

/**
 * Bulk-insert a JSON array into a DuckDB table using registerFileBuffer + read_json_auto.
 */
async function insertJsonArray(
  db: AsyncDuckDB,
  conn: AsyncDuckDBConnection,
  tableName: string,
  rows: unknown[],
): Promise<void> {
  const json = JSON.stringify(rows);
  const buffer = new TextEncoder().encode(json);
  const fileName = `${tableName}.json`;

  await db.registerFileBuffer(fileName, buffer);
  await conn.query(
    `INSERT INTO ${tableName} SELECT * FROM read_json_auto('${fileName}')`,
  );
  await db.dropFile(fileName);
}
