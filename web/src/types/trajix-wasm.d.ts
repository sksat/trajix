/**
 * Augment trajix-wasm module with types from trajix core (no tsify).
 *
 * Types defined in trajix-core don't have tsify, so wasm-pack's .d.ts
 * references them without defining them. We provide definitions here.
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

  type FixQuality = "Primary" | "GapFallback" | "Rejected";

  interface SatelliteSnapshot {
    time_ms: number;
    constellation: number;
    svid: number;
    azimuth_deg: number;
    elevation_deg: number;
    cn0_dbhz: number;
    used_in_fix: boolean;
  }

  interface DecimatedSample<V> {
    time_ms: number;
    value: V;
  }

  interface SensorXyz {
    x: number;
    y: number;
    z: number;
  }

  interface OrientationValue {
    yaw_deg: number;
    roll_deg: number;
    pitch_deg: number;
  }

  interface RotationValue {
    x: number;
    y: number;
    z: number;
    w: number;
  }
}
