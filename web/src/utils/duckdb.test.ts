/**
 * DuckDB schema, loader, and query tests.
 *
 * Uses @duckdb/node-api (Node.js native) for testing since
 * @duckdb/duckdb-wasm is browser-only. The SQL schema and queries
 * are identical between both clients.
 */
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { DuckDBInstance } from "@duckdb/node-api";
import { SCHEMA_SQL, TABLE_NAMES } from "./duckdbSchema";
import { mapFixRow } from "./duckdbLoader";
import {
  queryCn0TimeSeries,
  querySatCountTimeSeries,
  queryAccuracyTimeSeries,
  querySpeedTimeSeries,
  querySkyPlotSatellites,
  queryPrimaryFixes,
} from "./duckdbQueries";

// ────────────────────────────────────────────
// DuckDB Node.js setup
// ────────────────────────────────────────────

let instance: DuckDBInstance;
let conn: any;

beforeAll(async () => {
  instance = await DuckDBInstance.create(":memory:");
  conn = await instance.connect();
});

afterAll(() => {
  conn = null;
  instance = null as any;
});

/**
 * Adapter: wraps DuckDB node-api connection to match the query interface
 * expected by our query functions (which use duckdb-wasm's AsyncDuckDBConnection).
 */
/** Convert BigInt values to Number (DuckDB node-api returns BigInt for BIGINT columns). */
function coerceBigInt(value: unknown): unknown {
  if (typeof value === "bigint") return Number(value);
  return value;
}

function makeQueryAdapter(nodeConn: any) {
  return {
    async query(sql: string) {
      const reader = await nodeConn.runAndReadAll(sql);
      const columns = reader.columnNames();
      const rows = reader.getRows();
      return {
        toArray() {
          return rows.map((row: any[]) => {
            const obj: Record<string, any> = {};
            columns.forEach((col: string, i: number) => {
              obj[col] = coerceBigInt(row[i]);
            });
            return {
              toJSON() {
                return obj;
              },
              ...obj,
            };
          });
        },
      };
    },
  };
}

// ────────────────────────────────────────────
// Fix loader: undefined→null coercion
// ────────────────────────────────────────────

/** Column names of the DuckDB fix table. */
const FIX_COLUMNS = [
  "provider",
  "latitude_deg",
  "longitude_deg",
  "altitude_m",
  "speed_mps",
  "accuracy_m",
  "bearing_deg",
  "unix_time_ms",
  "speed_accuracy_mps",
  "bearing_accuracy_deg",
  "elapsed_realtime_ns",
  "vertical_accuracy_m",
  "mock_location",
  "num_used_signals",
  "solution_type",
  "quality",
];

describe("mapFixRow — undefined→null coercion", () => {
  /** Simulates serde-wasm-bindgen output: Option::None → JS undefined. */
  const wasmFix = {
    provider: "Gps",
    latitude_deg: 36.212,
    longitude_deg: 140.097,
    altitude_m: 281.3,
    speed_mps: undefined,
    accuracy_m: 3.79,
    bearing_deg: undefined,
    unix_time_ms: 1771641748000,
    speed_accuracy_mps: undefined,
    bearing_accuracy_deg: undefined,
    elapsed_realtime_ns: undefined,
    vertical_accuracy_m: undefined,
    mock_location: false,
    num_used_signals: undefined,
    solution_type: undefined,
  };

  it("JSON includes all 16 keys even when Option fields are undefined", () => {
    const row = mapFixRow(wasmFix as any, "Primary");
    const json = JSON.stringify(row);
    const parsed = JSON.parse(json);

    for (const key of FIX_COLUMNS) {
      expect(parsed, `missing key: ${key}`).toHaveProperty(key);
    }
    expect(Object.keys(parsed).length).toBe(FIX_COLUMNS.length);
  });

  it("undefined fields become null, not omitted", () => {
    const row = mapFixRow(wasmFix as any, "Primary");
    expect(row.num_used_signals).toBeNull();
    expect(row.solution_type).toBeNull();
    expect(row.speed_mps).toBeNull();
    expect(row.bearing_deg).toBeNull();
  });

  it("defined fields are preserved as-is", () => {
    const row = mapFixRow(wasmFix as any, "Primary");
    expect(row.provider).toBe("Gps");
    expect(row.latitude_deg).toBe(36.212);
    expect(row.altitude_m).toBe(281.3);
    expect(row.mock_location).toBe(false);
    expect(row.quality).toBe("Primary");
  });
});

describe("mapFixRow + read_json_auto integration", () => {
  it("read_json_auto inserts all 16 columns from coerced JSON", async () => {
    // Create a fresh table with same schema as fix
    await conn.run(`CREATE TABLE IF NOT EXISTS fix_json_test (
      provider VARCHAR, latitude_deg DOUBLE, longitude_deg DOUBLE,
      altitude_m DOUBLE, speed_mps DOUBLE, accuracy_m DOUBLE,
      bearing_deg DOUBLE, unix_time_ms BIGINT, speed_accuracy_mps DOUBLE,
      bearing_accuracy_deg DOUBLE, elapsed_realtime_ns BIGINT,
      vertical_accuracy_m DOUBLE, mock_location BOOLEAN,
      num_used_signals INTEGER, solution_type VARCHAR, quality VARCHAR
    )`);

    const rows = [
      mapFixRow(
        {
          provider: "Gps",
          latitude_deg: 36.212,
          longitude_deg: 140.097,
          altitude_m: 281.3,
          speed_mps: undefined,
          accuracy_m: 3.79,
          bearing_deg: undefined,
          unix_time_ms: 1771641748000,
          speed_accuracy_mps: undefined,
          bearing_accuracy_deg: undefined,
          elapsed_realtime_ns: undefined,
          vertical_accuracy_m: undefined,
          mock_location: false,
          num_used_signals: undefined,
          solution_type: undefined,
        } as any,
        "Primary",
      ),
    ];

    // Write JSON to temp file, load via read_json_auto
    const fs = await import("node:fs");
    const os = await import("node:os");
    const path = await import("node:path");
    const tmpFile = path.join(os.tmpdir(), "fix_json_test.json");
    fs.writeFileSync(tmpFile, JSON.stringify(rows));

    try {
      await conn.run(
        `INSERT INTO fix_json_test SELECT * FROM read_json_auto('${tmpFile}')`,
      );

      const reader = await conn.runAndReadAll(
        "SELECT COUNT(*)::INTEGER AS cnt FROM fix_json_test",
      );
      expect(reader.getRows()[0][0]).toBe(1);

      // Verify null fields round-trip
      const dataReader = await conn.runAndReadAll(
        "SELECT num_used_signals, solution_type FROM fix_json_test",
      );
      const row = dataReader.getRows()[0];
      expect(row[0]).toBeNull();
      expect(row[1]).toBeNull();
    } finally {
      fs.unlinkSync(tmpFile);
      await conn.run("DROP TABLE IF EXISTS fix_json_test");
    }
  });
});

// ────────────────────────────────────────────
// Schema tests
// ────────────────────────────────────────────

describe("DuckDB schema", () => {
  it("creates all tables without error", async () => {
    await conn.run(SCHEMA_SQL);

    for (const table of TABLE_NAMES) {
      const reader = await conn.runAndReadAll(
        `SELECT COUNT(*) AS cnt FROM ${table}`,
      );
      const rows = reader.getRows();
      expect(rows.length).toBe(1);
    }
  });

  it("fix table has expected columns", async () => {
    const reader = await conn.runAndReadAll("DESCRIBE fix");
    const columns = reader.getRows().map((r: any[]) => r[0]);
    expect(columns).toContain("provider");
    expect(columns).toContain("latitude_deg");
    expect(columns).toContain("unix_time_ms");
    expect(columns).toContain("quality");
    expect(columns).toContain("mock_location");
  });

  it("status table has expected columns", async () => {
    const reader = await conn.runAndReadAll("DESCRIBE status");
    const columns = reader.getRows().map((r: any[]) => r[0]);
    expect(columns).toContain("unix_time_ms");
    expect(columns).toContain("constellation");
    expect(columns).toContain("svid");
    expect(columns).toContain("azimuth_deg");
    expect(columns).toContain("elevation_deg");
    expect(columns).toContain("cn0_dbhz");
    expect(columns).toContain("used_in_fix");
  });
});

// ────────────────────────────────────────────
// Data loading + round-trip tests
// ────────────────────────────────────────────

describe("DuckDB data loading", () => {
  it("loads fix data and row counts match", async () => {
    await conn.run(`
      INSERT INTO fix VALUES
        ('Gps', 36.212, 140.097, 281.3, 0.0, 3.79, NULL, 1771641748000, 0.07, NULL, NULL, 3.66, false, NULL, NULL, 'Primary'),
        ('Flp', 36.213, 140.098, 282.0, 1.5, 4.0, 25.9, 1771641749000, NULL, NULL, NULL, 2.8, false, 12, NULL, 'Primary'),
        ('Nlp', 36.500, 140.500, NULL, NULL, 400.0, NULL, 1771641750000, NULL, NULL, NULL, NULL, false, NULL, NULL, 'Rejected')
    `);

    const reader = await conn.runAndReadAll(
      "SELECT COUNT(*)::INTEGER AS cnt FROM fix",
    );
    expect(reader.getRows()[0][0]).toBe(3);
  });

  it("fix values round-trip correctly", async () => {
    const reader = await conn.runAndReadAll(
      "SELECT provider, latitude_deg, altitude_m, mock_location, quality FROM fix ORDER BY unix_time_ms",
    );
    const rows = reader.getRows();

    // First fix (GPS, Primary)
    expect(rows[0][0]).toBe("Gps");
    expect(rows[0][1]).toBeCloseTo(36.212, 3);
    expect(rows[0][2]).toBeCloseTo(281.3, 1);
    expect(rows[0][3]).toBe(false);
    expect(rows[0][4]).toBe("Primary");

    // Third fix (NLP, null altitude, Rejected)
    expect(rows[2][0]).toBe("Nlp");
    expect(rows[2][2]).toBeNull();
    expect(rows[2][4]).toBe("Rejected");
  });

  it("loads satellite snapshots into status table", async () => {
    await conn.run(`
      INSERT INTO status VALUES
        (1771641748000, 1, 2, 192.285, 31.19, 25.7, true),
        (1771641748000, 3, 9, 10.0, 45.0, 28.4, true),
        (1771641749000, 1, 2, 192.5, 31.2, 30.0, true),
        (1771641749000, 6, 5, 270.0, 60.0, 35.0, false)
    `);

    const reader = await conn.runAndReadAll(
      "SELECT COUNT(*)::INTEGER AS cnt FROM status",
    );
    expect(reader.getRows()[0][0]).toBe(4);
  });

  it("status values round-trip correctly", async () => {
    const reader = await conn.runAndReadAll(
      "SELECT unix_time_ms, constellation, svid, cn0_dbhz, used_in_fix FROM status ORDER BY unix_time_ms, svid",
    );
    const rows = reader.getRows();

    // First: GPS svid=2
    expect(rows[0][1]).toBe(1); // GPS constellation
    expect(rows[0][2]).toBe(2); // svid
    expect(rows[0][3]).toBeCloseTo(25.7, 1);
    expect(rows[0][4]).toBe(true);
  });

  it("loads status epoch data", async () => {
    await conn.run(`
      INSERT INTO status_epoch VALUES
        (1771641748000, 27.05, 27.05, 2, 2),
        (1771641749000, 32.5, 30.0, 2, 1)
    `);

    const reader = await conn.runAndReadAll(
      "SELECT COUNT(*)::INTEGER AS cnt FROM status_epoch",
    );
    expect(reader.getRows()[0][0]).toBe(2);
  });

  it("loads fix epoch data", async () => {
    await conn.run(`
      INSERT INTO fix_epoch VALUES
        (1771641748000, 3.79, 3.66, 0.0),
        (1771641749000, 4.0, 2.8, 1.5)
    `);

    const reader = await conn.runAndReadAll(
      "SELECT COUNT(*)::INTEGER AS cnt FROM fix_epoch",
    );
    expect(reader.getRows()[0][0]).toBe(2);
  });
});

// ────────────────────────────────────────────
// Query tests (using data loaded above)
// ────────────────────────────────────────────

describe("DuckDB queries", () => {
  const adapter = (() => {
    // Lazy init: adapter created after conn is available
    let _adapter: any;
    return () => {
      if (!_adapter) _adapter = makeQueryAdapter(conn);
      return _adapter;
    };
  })();

  it("CN0 time series returns ordered rows", async () => {
    const rows = await queryCn0TimeSeries(adapter());
    expect(rows.length).toBe(2);
    expect(rows[0]!.time_ms).toBe(1771641748000);
    expect(rows[0]!.cn0_mean_all).toBeCloseTo(27.05, 1);
    expect(rows[1]!.time_ms).toBe(1771641749000);
  });

  it("satellite count time series", async () => {
    const rows = await querySatCountTimeSeries(adapter());
    expect(rows.length).toBe(2);
    expect(rows[0]!.num_visible).toBe(2);
    expect(rows[0]!.num_used).toBe(2);
    expect(rows[1]!.num_used).toBe(1);
  });

  it("accuracy time series", async () => {
    const rows = await queryAccuracyTimeSeries(adapter());
    expect(rows.length).toBe(2);
    expect(rows[0]!.accuracy_m).toBeCloseTo(3.79, 2);
    expect(rows[1]!.vertical_accuracy_m).toBeCloseTo(2.8, 1);
  });

  it("speed time series", async () => {
    const rows = await querySpeedTimeSeries(adapter());
    expect(rows.length).toBe(2);
    expect(rows[0]!.speed_mps).toBeCloseTo(0.0, 1);
    expect(rows[1]!.speed_mps).toBeCloseTo(1.5, 1);
  });

  it("sky plot satellites at specific time", async () => {
    const sats = await querySkyPlotSatellites(adapter(), 1771641748000, 1000);
    // At time 1771641748000: GPS svid=2 and GLONASS svid=9
    expect(sats.length).toBe(2);
    const gps = sats.find((s) => s.constellation === 1);
    expect(gps).toBeDefined();
    expect(gps!.svid).toBe(2);
    expect(gps!.azimuth_deg).toBeCloseTo(192.285, 2);
    const glonass = sats.find((s) => s.constellation === 3);
    expect(glonass).toBeDefined();
    expect(glonass!.svid).toBe(9);
  });

  it("sky plot deduplicates same satellite across windows", async () => {
    // At time 1771641748500 with 1000ms window: includes both epochs
    // GPS svid=2 appears at both 1771641748000 and 1771641749000
    // Should keep only highest CN0 (30.0 from second epoch)
    const sats = await querySkyPlotSatellites(adapter(), 1771641748500, 2000);
    const gpsSvid2 = sats.filter(
      (s) => s.constellation === 1 && s.svid === 2,
    );
    expect(gpsSvid2.length).toBe(1);
    expect(gpsSvid2[0]!.cn0_dbhz).toBeCloseTo(30.0, 1); // Higher CN0 kept
  });

  it("primary fixes filter works", async () => {
    const rows = await queryPrimaryFixes(adapter());
    expect(rows.length).toBe(2); // GPS + FLP, not NLP Rejected
    expect(rows.every((r) => r.quality === "Primary")).toBe(true);
  });
});
