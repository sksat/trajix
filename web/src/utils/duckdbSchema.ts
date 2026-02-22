/**
 * DuckDB table schema definitions for trajix data.
 */

export const SCHEMA_SQL = `
CREATE TABLE IF NOT EXISTS fix (
  provider VARCHAR,
  latitude_deg DOUBLE,
  longitude_deg DOUBLE,
  altitude_m DOUBLE,
  speed_mps DOUBLE,
  accuracy_m DOUBLE,
  bearing_deg DOUBLE,
  unix_time_ms BIGINT,
  speed_accuracy_mps DOUBLE,
  bearing_accuracy_deg DOUBLE,
  elapsed_realtime_ns BIGINT,
  vertical_accuracy_m DOUBLE,
  mock_location BOOLEAN,
  num_used_signals INTEGER,
  solution_type VARCHAR,
  quality VARCHAR
);

CREATE TABLE IF NOT EXISTS status (
  unix_time_ms BIGINT,
  constellation INTEGER,
  svid INTEGER,
  azimuth_deg DOUBLE,
  elevation_deg DOUBLE,
  cn0_dbhz DOUBLE,
  used_in_fix BOOLEAN
);

CREATE TABLE IF NOT EXISTS status_epoch (
  time_ms BIGINT,
  cn0_mean_all DOUBLE,
  cn0_mean_used DOUBLE,
  num_visible INTEGER,
  num_used INTEGER
);

CREATE TABLE IF NOT EXISTS fix_epoch (
  time_ms BIGINT,
  accuracy_m DOUBLE,
  vertical_accuracy_m DOUBLE,
  speed_mps DOUBLE
);
`;

/** Table names in creation order. */
export const TABLE_NAMES = ["fix", "status", "status_epoch", "fix_epoch"] as const;
