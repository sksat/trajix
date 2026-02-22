/**
 * Augment trajix-wasm module to declare FixRecord.
 *
 * FixRecord lives in trajix (no tsify), so the WASM package's .d.ts
 * references it without defining it. We provide the definition here.
 */
// This import makes this file a module, so `declare module` is treated as
// augmentation rather than replacement.
import "trajix-wasm";

declare module "trajix-wasm" {
  interface FixRecord {
    provider: string;
    latitude_deg: number;
    longitude_deg: number;
    altitude_m: number | null;
    speed_mps: number | null;
    accuracy_m: number | null;
    bearing_deg: number | null;
    unix_time_ms: number;
    speed_accuracy_mps: number | null;
    bearing_accuracy_deg: number | null;
    elapsed_realtime_ns: number | null;
    vertical_accuracy_m: number | null;
    mock_location: boolean;
    num_used_signals: number | null;
    vertical_speed_accuracy_mps: number | null;
    solution_type: string | null;
  }

  interface SatelliteSnapshotJs {
    time_ms: number;
    constellation: number;
    svid: number;
    azimuth_deg: number;
    elevation_deg: number;
    cn0_dbhz: number;
    used_in_fix: boolean;
  }

  /** Decimated sample (moved to trajix core, no tsify). */
  interface DecimatedSample<V> {
    time_ms: number;
    value: V;
  }
}
