use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::geo;
use crate::types::FixProvider;

/// Expected number of CSV fields in a Fix record (including the "Fix" prefix).
const FIX_FIELD_COUNT: usize = 17;

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

impl FixRecord {
    /// Parse a Fix record from a CSV line.
    ///
    /// The line should start with "Fix," and contain 17 comma-separated fields.
    pub fn parse(line: &str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != FIX_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type: "Fix",
                expected: FIX_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        let provider = FixProvider::from_str(fields[1])
            .ok_or_else(|| ParseError::UnknownProvider(fields[1].to_string()))?;

        Ok(FixRecord {
            provider,
            latitude_deg: parse_f64(fields[2], "LatitudeDegrees")?,
            longitude_deg: parse_f64(fields[3], "LongitudeDegrees")?,
            altitude_m: parse_optional_f64(fields[4]),
            speed_mps: parse_optional_f64(fields[5]),
            accuracy_m: parse_optional_f64(fields[6]),
            bearing_deg: parse_optional_f64(fields[7]),
            unix_time_ms: parse_i64(fields[8], "UnixTimeMillis")?,
            speed_accuracy_mps: parse_optional_f64(fields[9]),
            bearing_accuracy_deg: parse_optional_f64(fields[10]),
            elapsed_realtime_ns: parse_optional_i64(fields[11]),
            vertical_accuracy_m: parse_optional_f64(fields[12]),
            mock_location: fields[13] == "1",
            num_used_signals: parse_optional_u32(fields[14]),
            vertical_speed_accuracy_mps: parse_optional_f64(fields[15]),
            solution_type: if fields[16].is_empty() {
                None
            } else {
                Some(fields[16].to_string())
            },
        })
    }
}

impl FixRecord {
    /// Haversine distance to another fix in meters.
    pub fn distance_to(&self, other: &Self) -> f64 {
        geo::haversine_distance_m(
            self.latitude_deg,
            self.longitude_deg,
            other.latitude_deg,
            other.longitude_deg,
        )
    }

    /// Time difference in seconds (other - self). Positive if other is later.
    pub fn time_delta_s(&self, other: &Self) -> f64 {
        (other.unix_time_ms - self.unix_time_ms) as f64 / 1000.0
    }

    /// Implied ground speed between two fixes in m/s.
    ///
    /// Returns `None` if the time delta is zero.
    pub fn speed_between(&self, other: &Self) -> Option<f64> {
        let dt = self.time_delta_s(other).abs();
        if dt == 0.0 {
            return None;
        }
        Some(self.distance_to(other) / dt)
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

fn parse_optional_f64(s: &str) -> Option<f64> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_i64(s: &str) -> Option<i64> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_u32(s: &str) -> Option<u32> {
    if s.is_empty() { None } else { s.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parse_normal_gps_fix() {
        let content = load_fixture("fix_normal_gps.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            assert_eq!(fix.provider, FixProvider::Gps);
            assert!((fix.latitude_deg - 36.212).abs() < 0.001);
            assert!((fix.longitude_deg - 140.097).abs() < 0.001);
            assert!(fix.altitude_m.is_some());
            assert!(fix.accuracy_m.is_some());
            assert!(!fix.mock_location);
        }
    }

    #[test]
    fn parse_normal_gps_fix_field_values() {
        let content = load_fixture("fix_normal_gps.txt");
        let line = content.lines().next().unwrap();
        let fix = FixRecord::parse(line).unwrap();

        assert_eq!(fix.provider, FixProvider::Gps);
        assert!((fix.latitude_deg - 36.2120566600).abs() < 1e-9);
        assert!((fix.longitude_deg - 140.0965061400).abs() < 1e-9);
        assert!((fix.altitude_m.unwrap() - 281.32696533203125).abs() < 1e-6);
        assert!((fix.speed_mps.unwrap() - 0.0).abs() < 1e-9);
        assert!((fix.accuracy_m.unwrap() - 3.7900925).abs() < 1e-6);
        assert!(fix.bearing_deg.is_none()); // empty in this record
        assert_eq!(fix.unix_time_ms, 1771641748000);
        assert!((fix.speed_accuracy_mps.unwrap() - 0.07064216).abs() < 1e-6);
        assert!(fix.bearing_accuracy_deg.is_none()); // empty
        assert_eq!(fix.elapsed_realtime_ns.unwrap(), 2091905471128467);
        assert!((fix.vertical_accuracy_m.unwrap() - 3.659059).abs() < 1e-5);
        assert!(!fix.mock_location);
        assert!(fix.num_used_signals.is_none());
        assert!(fix.vertical_speed_accuracy_mps.is_none());
        assert!(fix.solution_type.is_none());
    }

    #[test]
    fn parse_gps_fix_with_bearing() {
        let content = load_fixture("fix_normal_gps.txt");
        // Third line has bearing
        let line = content.lines().nth(2).unwrap();
        let fix = FixRecord::parse(line).unwrap();

        assert!((fix.bearing_deg.unwrap() - 265.4).abs() < 0.1);
        assert!((fix.speed_mps.unwrap() - 0.26).abs() < 0.01);
        assert!(fix.bearing_accuracy_deg.is_some());
    }

    #[test]
    fn parse_normal_flp_fix() {
        let content = load_fixture("fix_normal_flp.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            assert_eq!(fix.provider, FixProvider::Flp);
            assert!(fix.latitude_deg > 36.0);
            assert!(fix.longitude_deg > 140.0);
        }
    }

    #[test]
    fn parse_nlp_empty_fields() {
        let content = load_fixture("fix_nlp_empty_fields.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            assert_eq!(fix.provider, FixProvider::Nlp);
            assert!(fix.altitude_m.is_none(), "NLP should have empty altitude");
            assert!(fix.speed_mps.is_none(), "NLP should have empty speed");
            assert!(fix.bearing_deg.is_none());
            assert!(fix.speed_accuracy_mps.is_none());
            assert!(fix.bearing_accuracy_deg.is_none());
        }
    }

    #[test]
    fn parse_nlp_accuracy_400() {
        let content = load_fixture("fix_nlp_accuracy_400.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            assert_eq!(fix.provider, FixProvider::Nlp);
            assert!((fix.accuracy_m.unwrap() - 400.0).abs() < 0.1);
        }
    }

    #[test]
    fn parse_high_speed_fix() {
        let content = load_fixture("fix_high_speed.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            assert_eq!(fix.provider, FixProvider::Gps);
            assert!(fix.speed_mps.unwrap() > 10.0);
            assert!(fix.bearing_deg.is_some());
        }
    }

    #[test]
    fn parse_mid_file_fix() {
        let content = load_fixture("fix_mid_file.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let fix = FixRecord::parse(line).unwrap();
            // Mid-file location is different from start
            assert!(fix.latitude_deg > 36.0);
            assert!(fix.unix_time_ms > 1771641748000);
        }
    }

    #[test]
    fn parse_fix_wrong_field_count() {
        let result = FixRecord::parse("Fix,GPS,36.0,140.0");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::FieldCount {
                record_type,
                expected,
                actual,
            } => {
                assert_eq!(record_type, "Fix");
                assert_eq!(expected, 17);
                assert_eq!(actual, 4);
            }
            _ => panic!("expected FieldCount error"),
        }
    }

    #[test]
    fn parse_fix_unknown_provider() {
        let line = "Fix,UNKNOWN,36.0,140.0,100.0,0.0,5.0,,1234567890,,,123456789,1.0,0,,,";
        let result = FixRecord::parse(line);
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::UnknownProvider(p) => assert_eq!(p, "UNKNOWN"),
            _ => panic!("expected UnknownProvider error"),
        }
    }

    // ── distance_to / time_delta_s / speed_between ──

    fn make_fix(lat: f64, lon: f64, time_ms: i64) -> FixRecord {
        FixRecord {
            provider: FixProvider::Gps,
            latitude_deg: lat,
            longitude_deg: lon,
            altitude_m: None,
            speed_mps: None,
            accuracy_m: None,
            bearing_deg: None,
            unix_time_ms: time_ms,
            speed_accuracy_mps: None,
            bearing_accuracy_deg: None,
            elapsed_realtime_ns: None,
            vertical_accuracy_m: None,
            mock_location: false,
            num_used_signals: None,
            vertical_speed_accuracy_mps: None,
            solution_type: None,
        }
    }

    #[test]
    fn distance_to_same_point() {
        let a = make_fix(36.212, 140.097, 1000);
        assert!(a.distance_to(&a) < 1e-10);
    }

    #[test]
    fn distance_to_known_points() {
        // ~111m apart (0.001 deg latitude)
        let a = make_fix(36.0, 140.0, 1000);
        let b = make_fix(36.001, 140.0, 2000);
        let dist = a.distance_to(&b);
        assert!((dist - 111.0).abs() < 2.0, "expected ~111m, got {dist:.1}m");
    }

    #[test]
    fn distance_to_is_symmetric() {
        let a = make_fix(35.6812, 139.7671, 1000);
        let b = make_fix(34.7024, 135.4959, 2000);
        assert!((a.distance_to(&b) - b.distance_to(&a)).abs() < 1e-6);
    }

    #[test]
    fn time_delta_s_positive() {
        let a = make_fix(36.0, 140.0, 1000);
        let b = make_fix(36.0, 140.0, 3500);
        assert!((a.time_delta_s(&b) - 2.5).abs() < 1e-10);
    }

    #[test]
    fn time_delta_s_negative() {
        let a = make_fix(36.0, 140.0, 5000);
        let b = make_fix(36.0, 140.0, 3000);
        assert!((a.time_delta_s(&b) - (-2.0)).abs() < 1e-10);
    }

    #[test]
    fn speed_between_known() {
        // 111m in 1s = 111 m/s
        let a = make_fix(36.0, 140.0, 0);
        let b = make_fix(36.001, 140.0, 1000);
        let speed = a.speed_between(&b).unwrap();
        assert!((speed - 111.0).abs() < 2.0, "expected ~111 m/s, got {speed:.1}");
    }

    #[test]
    fn speed_between_zero_dt() {
        let a = make_fix(36.0, 140.0, 1000);
        let b = make_fix(36.001, 140.0, 1000);
        assert!(a.speed_between(&b).is_none());
    }

    #[test]
    fn speed_between_is_symmetric() {
        let a = make_fix(36.0, 140.0, 0);
        let b = make_fix(36.001, 140.0, 1000);
        let s1 = a.speed_between(&b).unwrap();
        let s2 = b.speed_between(&a).unwrap();
        assert!((s1 - s2).abs() < 1e-6);
    }

    #[test]
    fn distance_to_fixture_data() {
        // Use real fixture data: two consecutive GPS fixes
        let content = load_fixture("fix_normal_gps.txt");
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        if lines.len() >= 2 {
            let a = FixRecord::parse(lines[0]).unwrap();
            let b = FixRecord::parse(lines[1]).unwrap();
            let dist = a.distance_to(&b);
            // Consecutive fixes should be close (< 100m typically)
            assert!(dist < 10000.0, "consecutive fixes should be close, got {dist:.1}m");
        }
    }
}
