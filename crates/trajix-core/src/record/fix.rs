use serde::{Deserialize, Serialize};

use crate::types::FixProvider;

/// A position fix record from GNSS Logger.
///
/// 17 CSV fields: Fix,Provider,Lat,Lon,Alt,Speed,Accuracy,Bearing,
/// UnixTimeMs,SpeedAcc,BearingAcc,ElapsedRealtimeNs,VerticalAcc,
/// MockLocation,NumUsedSignals,VerticalSpeedAcc,SolutionType
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixRecord {
    pub provider: FixProvider,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub accuracy_m: Option<f64>,
    pub bearing_deg: Option<f64>,
    pub unix_time_ms: i64,
    pub speed_accuracy_mps: Option<f64>,
    pub bearing_accuracy_deg: Option<f64>,
    pub elapsed_realtime_ns: Option<i64>,
    pub vertical_accuracy_m: Option<f64>,
    pub mock_location: bool,
    pub num_used_signals: Option<u32>,
    pub vertical_speed_accuracy_mps: Option<f64>,
    pub solution_type: Option<String>,
}
