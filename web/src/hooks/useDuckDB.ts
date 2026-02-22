/**
 * React hook for DuckDB-wasm integration.
 *
 * Manages DuckDB lifecycle: initialization, data loading, and query access.
 */
import { useState, useCallback } from "react";
import type { AsyncDuckDBConnection } from "@duckdb/duckdb-wasm";
import type { ProcessingResult } from "../types/gnss";
import { initDuckDB, getConnection } from "../utils/duckdb";
import { loadResultIntoDuckDB } from "../utils/duckdbLoader";

export type DuckDBStatus = "idle" | "loading" | "ready" | "error";

export interface UseDuckDB {
  status: DuckDBStatus;
  conn: AsyncDuckDBConnection | null;
  error: string | null;
  loadResult: (result: ProcessingResult) => Promise<void>;
}

export function useDuckDB(): UseDuckDB {
  const [status, setStatus] = useState<DuckDBStatus>("idle");
  const [conn, setConn] = useState<AsyncDuckDBConnection | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadResult = useCallback(async (result: ProcessingResult) => {
    setStatus("loading");
    setError(null);
    try {
      const db = await initDuckDB();
      const connection = await getConnection();
      await loadResultIntoDuckDB(db, connection, result);
      setConn(connection);
      setStatus("ready");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg);
      setStatus("error");
      console.error("DuckDB loading failed:", e);
    }
  }, []);

  return { status, conn, error, loadResult };
}
