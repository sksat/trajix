/**
 * DuckDB-wasm initialization (lazy singleton).
 *
 * Provides a shared AsyncDuckDB instance and connection.
 * The WASM bundle is loaded lazily on first call.
 */
import * as duckdb from "@duckdb/duckdb-wasm";

let db: duckdb.AsyncDuckDB | null = null;
let conn: duckdb.AsyncDuckDBConnection | null = null;

/**
 * Initialize (or return existing) DuckDB-wasm instance.
 *
 * In browser: uses CDN-hosted WASM bundles via MANUAL_BUNDLES.
 * In Node.js (vitest): uses the bundled Node worker.
 */
export async function initDuckDB(): Promise<duckdb.AsyncDuckDB> {
  if (db) return db;

  const bundle = await duckdb.selectBundle(duckdb.getJsDelivrBundles());
  const worker = await duckdb.createWorker(bundle.mainWorker!);
  const logger = new duckdb.ConsoleLogger();
  db = new duckdb.AsyncDuckDB(logger, worker);
  await db.instantiate(bundle.mainModule, bundle.pthreadWorker);
  return db;
}

/**
 * Get (or create) a shared DuckDB connection.
 */
export async function getConnection(): Promise<duckdb.AsyncDuckDBConnection> {
  if (conn) return conn;
  const database = await initDuckDB();
  conn = await database.connect();
  return conn;
}

/**
 * Get the underlying AsyncDuckDB instance (must be initialized first).
 */
export function getDB(): duckdb.AsyncDuckDB | null {
  return db;
}

/**
 * Reset the singleton (for testing).
 */
export async function resetDuckDB(): Promise<void> {
  if (conn) {
    await conn.close();
    conn = null;
  }
  if (db) {
    await db.terminate();
    db = null;
  }
}
