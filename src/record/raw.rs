use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::types::{CodeType, ConstellationType};

/// Expected number of CSV fields in a Raw record (including the "Raw" prefix).
const RAW_FIELD_COUNT: usize = 54;

/// A raw GNSS measurement record from GNSS Logger.
///
/// 54 CSV fields: Raw,utcTimeMillis,TimeNanos,LeapSecond,
/// TimeUncertaintyNanos,FullBiasNanos,BiasNanos,BiasUncertaintyNanos,
/// DriftNanosPerSecond,DriftUncertaintyNanosPerSecond,
/// HardwareClockDiscontinuityCount,Svid,TimeOffsetNanos,State,
/// ReceivedSvTimeNanos,ReceivedSvTimeUncertaintyNanos,Cn0DbHz,
/// PseudorangeRateMetersPerSecond,PseudorangeRateUncertaintyMetersPerSecond,
/// AccumulatedDeltaRangeState,AccumulatedDeltaRangeMeters,
/// AccumulatedDeltaRangeUncertaintyMeters,CarrierFrequencyHz,
/// CarrierCycles,CarrierPhase,CarrierPhaseUncertainty,
/// MultipathIndicator,SnrInDb,ConstellationType,AgcDb,BasebandCn0DbHz,
/// FullInterSignalBiasNanos,FullInterSignalBiasUncertaintyNanos,
/// SatelliteInterSignalBiasNanos,SatelliteInterSignalBiasUncertaintyNanos,
/// CodeType,ChipsetElapsedRealtimeNanos,IsFullTracking,
/// SvPositionEcefXMeters,SvPositionEcefYMeters,SvPositionEcefZMeters,
/// SvVelocityEcefXMetersPerSecond,SvVelocityEcefYMetersPerSecond,
/// SvVelocityEcefZMetersPerSecond,SvClockBiasMeters,
/// SvClockDriftMetersPerSecond,KlobucharAlpha0,KlobucharAlpha1,
/// KlobucharAlpha2,KlobucharAlpha3,KlobucharBeta0,KlobucharBeta1,
/// KlobucharBeta2,KlobucharBeta3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRecord {
    // Clock fields
    pub utc_time_ms: i64,
    pub time_nanos: i64,
    pub leap_second: Option<i32>,
    pub time_uncertainty_nanos: Option<f64>,
    pub full_bias_nanos: i64,
    pub bias_nanos: f64,
    pub bias_uncertainty_nanos: f64,
    pub drift_nanos_per_second: f64,
    pub drift_uncertainty_nanos_per_second: f64,
    pub hardware_clock_discontinuity_count: i32,

    // Satellite measurement fields
    pub svid: u32,
    pub time_offset_nanos: f64,
    pub state: u32,
    pub received_sv_time_nanos: i64,
    pub received_sv_time_uncertainty_nanos: i64,
    pub cn0_dbhz: f64,
    pub pseudorange_rate_mps: f64,
    pub pseudorange_rate_uncertainty_mps: f64,
    pub accumulated_delta_range_state: u32,
    pub accumulated_delta_range_meters: f64,
    pub accumulated_delta_range_uncertainty_meters: f64,
    pub carrier_frequency_hz: Option<f64>,
    pub carrier_cycles: Option<i64>,
    pub carrier_phase: Option<f64>,
    pub carrier_phase_uncertainty: Option<f64>,
    pub multipath_indicator: u32,
    pub snr_in_db: Option<f64>,
    pub constellation: ConstellationType,
    pub agc_db: Option<f64>,
    pub baseband_cn0_dbhz: Option<f64>,

    // Inter-signal bias
    pub full_inter_signal_bias_nanos: Option<f64>,
    pub full_inter_signal_bias_uncertainty_nanos: Option<f64>,
    pub satellite_inter_signal_bias_nanos: Option<f64>,
    pub satellite_inter_signal_bias_uncertainty_nanos: Option<f64>,

    // Code type and timing
    pub code_type: Option<CodeType>,
    pub chipset_elapsed_realtime_nanos: Option<i64>,
    pub is_full_tracking: Option<bool>,

    // Satellite ECEF position (empty for QZSS)
    pub sv_position_ecef_x_m: Option<f64>,
    pub sv_position_ecef_y_m: Option<f64>,
    pub sv_position_ecef_z_m: Option<f64>,
    pub sv_velocity_ecef_x_mps: Option<f64>,
    pub sv_velocity_ecef_y_mps: Option<f64>,
    pub sv_velocity_ecef_z_mps: Option<f64>,
    pub sv_clock_bias_m: Option<f64>,
    pub sv_clock_drift_mps: Option<f64>,

    // Klobuchar ionospheric model parameters
    pub klobuchar_alpha0: Option<f64>,
    pub klobuchar_alpha1: Option<f64>,
    pub klobuchar_alpha2: Option<f64>,
    pub klobuchar_alpha3: Option<f64>,
    pub klobuchar_beta0: Option<f64>,
    pub klobuchar_beta1: Option<f64>,
    pub klobuchar_beta2: Option<f64>,
    pub klobuchar_beta3: Option<f64>,
}

impl RawRecord {
    /// Parse a Raw record from a CSV line.
    ///
    /// The line should start with "Raw," and contain 54 comma-separated fields.
    pub fn parse(line: &str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != RAW_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type: "Raw",
                expected: RAW_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        let constellation_u8 = parse_u32(fields[28], "ConstellationType")? as u8;
        let constellation = ConstellationType::from_u8(constellation_u8);

        Ok(RawRecord {
            utc_time_ms: parse_i64(fields[1], "utcTimeMillis")?,
            time_nanos: parse_i64(fields[2], "TimeNanos")?,
            leap_second: parse_optional_i32(fields[3]),
            time_uncertainty_nanos: parse_optional_f64(fields[4]),
            full_bias_nanos: parse_i64(fields[5], "FullBiasNanos")?,
            bias_nanos: parse_f64(fields[6], "BiasNanos")?,
            bias_uncertainty_nanos: parse_f64(fields[7], "BiasUncertaintyNanos")?,
            drift_nanos_per_second: parse_f64(fields[8], "DriftNanosPerSecond")?,
            drift_uncertainty_nanos_per_second: parse_f64(
                fields[9],
                "DriftUncertaintyNanosPerSecond",
            )?,
            hardware_clock_discontinuity_count: parse_i32(
                fields[10],
                "HardwareClockDiscontinuityCount",
            )?,
            svid: parse_u32(fields[11], "Svid")?,
            time_offset_nanos: parse_f64(fields[12], "TimeOffsetNanos")?,
            state: parse_u32(fields[13], "State")?,
            received_sv_time_nanos: parse_i64(fields[14], "ReceivedSvTimeNanos")?,
            received_sv_time_uncertainty_nanos: parse_i64(
                fields[15],
                "ReceivedSvTimeUncertaintyNanos",
            )?,
            cn0_dbhz: parse_f64(fields[16], "Cn0DbHz")?,
            pseudorange_rate_mps: parse_f64(fields[17], "PseudorangeRateMetersPerSecond")?,
            pseudorange_rate_uncertainty_mps: parse_f64(
                fields[18],
                "PseudorangeRateUncertaintyMetersPerSecond",
            )?,
            accumulated_delta_range_state: parse_u32(fields[19], "AccumulatedDeltaRangeState")?,
            accumulated_delta_range_meters: parse_f64(fields[20], "AccumulatedDeltaRangeMeters")?,
            accumulated_delta_range_uncertainty_meters: parse_f64(
                fields[21],
                "AccumulatedDeltaRangeUncertaintyMeters",
            )?,
            carrier_frequency_hz: parse_optional_f64(fields[22]),
            carrier_cycles: parse_optional_i64(fields[23]),
            carrier_phase: parse_optional_f64(fields[24]),
            carrier_phase_uncertainty: parse_optional_f64(fields[25]),
            multipath_indicator: parse_u32(fields[26], "MultipathIndicator")?,
            snr_in_db: parse_optional_f64(fields[27]),
            constellation,
            agc_db: parse_optional_f64(fields[29]),
            baseband_cn0_dbhz: parse_optional_f64(fields[30]),
            full_inter_signal_bias_nanos: parse_optional_f64(fields[31]),
            full_inter_signal_bias_uncertainty_nanos: parse_optional_f64(fields[32]),
            satellite_inter_signal_bias_nanos: parse_optional_f64(fields[33]),
            satellite_inter_signal_bias_uncertainty_nanos: parse_optional_f64(fields[34]),
            code_type: if fields[35].is_empty() {
                None
            } else {
                CodeType::from_str(fields[35])
            },
            chipset_elapsed_realtime_nanos: parse_optional_i64(fields[36]),
            is_full_tracking: parse_optional_bool(fields[37]),
            sv_position_ecef_x_m: parse_optional_f64(fields[38]),
            sv_position_ecef_y_m: parse_optional_f64(fields[39]),
            sv_position_ecef_z_m: parse_optional_f64(fields[40]),
            sv_velocity_ecef_x_mps: parse_optional_f64(fields[41]),
            sv_velocity_ecef_y_mps: parse_optional_f64(fields[42]),
            sv_velocity_ecef_z_mps: parse_optional_f64(fields[43]),
            sv_clock_bias_m: parse_optional_f64(fields[44]),
            sv_clock_drift_mps: parse_optional_f64(fields[45]),
            klobuchar_alpha0: parse_optional_f64(fields[46]),
            klobuchar_alpha1: parse_optional_f64(fields[47]),
            klobuchar_alpha2: parse_optional_f64(fields[48]),
            klobuchar_alpha3: parse_optional_f64(fields[49]),
            klobuchar_beta0: parse_optional_f64(fields[50]),
            klobuchar_beta1: parse_optional_f64(fields[51]),
            klobuchar_beta2: parse_optional_f64(fields[52]),
            klobuchar_beta3: parse_optional_f64(fields[53]),
        })
    }
}

fn parse_f64(s: &str, field: &'static str) -> Result<f64, ParseError> {
    s.parse::<f64>().map_err(|e| ParseError::FieldParse {
        field,
        source: Box::new(e),
    })
}

fn parse_i64(s: &str, field: &'static str) -> Result<i64, ParseError> {
    s.parse::<i64>().map_err(|e| ParseError::FieldParse {
        field,
        source: Box::new(e),
    })
}

fn parse_i32(s: &str, field: &'static str) -> Result<i32, ParseError> {
    s.parse::<i32>().map_err(|e| ParseError::FieldParse {
        field,
        source: Box::new(e),
    })
}

fn parse_u32(s: &str, field: &'static str) -> Result<u32, ParseError> {
    s.parse::<u32>().map_err(|e| ParseError::FieldParse {
        field,
        source: Box::new(e),
    })
}

fn parse_optional_f64(s: &str) -> Option<f64> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_i64(s: &str) -> Option<i64> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_i32(s: &str) -> Option<i32> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_bool(s: &str) -> Option<bool> {
    if s.is_empty() {
        None
    } else {
        Some(s == "1" || s == "true")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parse_gps_raw_with_ecef() {
        let content = load_fixture("raw_with_ecef.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let r = RawRecord::parse(line).unwrap();
            assert_eq!(r.constellation, ConstellationType::Gps);
            assert!(r.sv_position_ecef_x_m.is_some());
            assert!(r.sv_position_ecef_y_m.is_some());
            assert!(r.sv_position_ecef_z_m.is_some());
            assert!(r.sv_velocity_ecef_x_mps.is_some());
            assert!(r.sv_velocity_ecef_y_mps.is_some());
            assert!(r.sv_velocity_ecef_z_mps.is_some());
            assert!(r.sv_clock_bias_m.is_some());
            assert!(r.sv_clock_drift_mps.is_some());
        }
    }

    #[test]
    fn parse_gps_raw_field_values() {
        let content = load_fixture("raw_with_ecef.txt");
        let line = content.lines().next().unwrap();
        let r = RawRecord::parse(line).unwrap();

        assert_eq!(r.utc_time_ms, 1771641747420);
        assert_eq!(r.time_nanos, 2609836088000000);
        assert_eq!(r.leap_second, Some(18));
        assert_eq!(r.full_bias_nanos, -1453067129332630190);
        assert_eq!(r.svid, 2);
        assert_eq!(r.constellation, ConstellationType::Gps);
        assert!((r.cn0_dbhz - 25.7).abs() < 0.01);
        assert!((r.pseudorange_rate_mps - (-629.548583984375)).abs() < 1e-6);
        assert_eq!(r.code_type, Some(CodeType::C));
        assert!(r.carrier_cycles.is_none());
        assert!(r.carrier_phase.is_none());

        // ECEF position
        assert!((r.sv_position_ecef_x_m.unwrap() - (-16950189.95636873)).abs() < 1e-3);
        assert!((r.sv_position_ecef_y_m.unwrap() - 19462945.022213325).abs() < 1e-3);

        // Klobuchar
        assert!(r.klobuchar_alpha0.is_some());
        assert!((r.klobuchar_beta0.unwrap() - 122880.0).abs() < 0.1);
    }

    #[test]
    fn parse_qzss_raw_without_ecef() {
        let content = load_fixture("raw_without_ecef.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let r = RawRecord::parse(line).unwrap();
            assert_eq!(r.constellation, ConstellationType::Qzss);
            assert!(
                r.sv_position_ecef_x_m.is_none(),
                "QZSS should have no ECEF X"
            );
            assert!(
                r.sv_position_ecef_y_m.is_none(),
                "QZSS should have no ECEF Y"
            );
            assert!(
                r.sv_position_ecef_z_m.is_none(),
                "QZSS should have no ECEF Z"
            );
            assert!(r.sv_velocity_ecef_x_mps.is_none());
            assert!(r.sv_velocity_ecef_y_mps.is_none());
            assert!(r.sv_velocity_ecef_z_mps.is_none());
            assert!(r.sv_clock_bias_m.is_none());
            assert!(r.sv_clock_drift_mps.is_none());
        }
    }

    #[test]
    fn parse_qzss_has_klobuchar() {
        let content = load_fixture("raw_without_ecef.txt");
        let line = content.lines().next().unwrap();
        let r = RawRecord::parse(line).unwrap();

        // QZSS still has Klobuchar parameters even without ECEF
        assert!(r.klobuchar_alpha0.is_some());
        assert!(r.klobuchar_beta3.is_some());
    }

    #[test]
    fn parse_raw_wrong_field_count() {
        let result = RawRecord::parse("Raw,123,456");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::FieldCount {
                record_type,
                expected,
                actual,
            } => {
                assert_eq!(record_type, "Raw");
                assert_eq!(expected, 54);
                assert_eq!(actual, 3);
            }
            _ => panic!("expected FieldCount error"),
        }
    }
}
