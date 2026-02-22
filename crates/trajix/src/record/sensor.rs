use serde::{Deserialize, Serialize};

use crate::error::ParseError;

// Field counts (including record-type prefix)
const UNCAL_FIELD_COUNT: usize = 10;
const ORIENTATION_FIELD_COUNT: usize = 7;
const GAME_ROTATION_FIELD_COUNT: usize = 7;

/// Uncalibrated 3-axis sensor record (accelerometer, gyroscope, or magnetometer).
///
/// 10 CSV fields: Type,utcTimeMillis,elapsedRealtimeNanos,
/// X,Y,Z,BiasX,BiasY,BiasZ,CalibrationAccuracy
///
/// Used for UncalAccel (m/s²), UncalGyro (rad/s), UncalMag (µT).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncalibratedSensorRecord {
    pub utc_time_ms: i64,
    pub elapsed_realtime_ns: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub bias_x: f64,
    pub bias_y: f64,
    pub bias_z: f64,
    pub calibration_accuracy: u32,
}

impl UncalibratedSensorRecord {
    /// Parse an uncalibrated sensor record (UncalAccel, UncalGyro, or UncalMag).
    pub fn parse(line: &str, record_type: &'static str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != UNCAL_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type,
                expected: UNCAL_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        Ok(UncalibratedSensorRecord {
            utc_time_ms: parse_i64(fields[1], "utcTimeMillis")?,
            elapsed_realtime_ns: parse_i64(fields[2], "elapsedRealtimeNanos")?,
            x: parse_f64(fields[3], "X")?,
            y: parse_f64(fields[4], "Y")?,
            z: parse_f64(fields[5], "Z")?,
            bias_x: parse_f64(fields[6], "BiasX")?,
            bias_y: parse_f64(fields[7], "BiasY")?,
            bias_z: parse_f64(fields[8], "BiasZ")?,
            calibration_accuracy: parse_u32(fields[9], "CalibrationAccuracy")?,
        })
    }
}

/// Orientation record in degrees (yaw, roll, pitch).
///
/// 7 CSV fields: OrientationDeg,utcTimeMillis,elapsedRealtimeNanos,
/// yawDeg,rollDeg,pitchDeg,CalibrationAccuracy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrientationRecord {
    pub utc_time_ms: i64,
    pub elapsed_realtime_ns: i64,
    pub yaw_deg: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub calibration_accuracy: u32,
}

impl OrientationRecord {
    pub fn parse(line: &str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != ORIENTATION_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type: "OrientationDeg",
                expected: ORIENTATION_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        Ok(OrientationRecord {
            utc_time_ms: parse_i64(fields[1], "utcTimeMillis")?,
            elapsed_realtime_ns: parse_i64(fields[2], "elapsedRealtimeNanos")?,
            yaw_deg: parse_f64(fields[3], "yawDeg")?,
            roll_deg: parse_f64(fields[4], "rollDeg")?,
            pitch_deg: parse_f64(fields[5], "pitchDeg")?,
            calibration_accuracy: parse_u32(fields[6], "CalibrationAccuracy")?,
        })
    }
}

/// Game rotation vector (quaternion without magnetometer reference).
///
/// 7 CSV fields: GameRotationVector,utcTimeMillis,elapsedRealtimeNanos,x,y,z,w
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRotationVectorRecord {
    pub utc_time_ms: i64,
    pub elapsed_realtime_ns: i64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl GameRotationVectorRecord {
    pub fn parse(line: &str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != GAME_ROTATION_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type: "GameRotationVector",
                expected: GAME_ROTATION_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        Ok(GameRotationVectorRecord {
            utc_time_ms: parse_i64(fields[1], "utcTimeMillis")?,
            elapsed_realtime_ns: parse_i64(fields[2], "elapsedRealtimeNanos")?,
            x: parse_f64(fields[3], "x")?,
            y: parse_f64(fields[4], "y")?,
            z: parse_f64(fields[5], "z")?,
            w: parse_f64(fields[6], "w")?,
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

fn parse_u32(s: &str, field: &'static str) -> Result<u32, ParseError> {
    s.parse::<u32>().map_err(|e| ParseError::FieldParse {
        field,
        source: Box::new(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    fn fixture_line(index: usize) -> String {
        let content = load_fixture("sensor_all_types.txt");
        content.lines().nth(index).unwrap().to_string()
    }

    #[test]
    fn parse_uncal_accel() {
        let line = fixture_line(0);
        let r = UncalibratedSensorRecord::parse(&line, "UncalAccel").unwrap();

        assert_eq!(r.utc_time_ms, 1771641748217);
        assert_eq!(r.elapsed_realtime_ns, 2091904808688268);
        assert!((r.x - 0.18185452).abs() < 1e-7);
        assert!((r.y - 6.424729).abs() < 1e-5);
        assert!((r.z - 7.4105716).abs() < 1e-5);
        assert!((r.bias_x - 0.0).abs() < 1e-9);
        assert!((r.bias_y - 0.0).abs() < 1e-9);
        assert!((r.bias_z - 0.0).abs() < 1e-9);
        assert_eq!(r.calibration_accuracy, 3);
    }

    #[test]
    fn parse_uncal_gyro() {
        let line = fixture_line(1);
        let r = UncalibratedSensorRecord::parse(&line, "UncalGyro").unwrap();

        assert_eq!(r.utc_time_ms, 1771641748215);
        assert!((r.x - (-0.027488915)).abs() < 1e-8);
        assert!((r.y - (-0.12828161)).abs() < 1e-7);
        assert!((r.z - (-0.0073303776)).abs() < 1e-9);
        // Drift values (stored as bias fields)
        assert!((r.bias_x - (-0.003136047)).abs() < 1e-8);
        assert!((r.bias_y - (-0.0021078899)).abs() < 1e-9);
        assert!((r.bias_z - (-0.00014410952)).abs() < 1e-10);
        assert_eq!(r.calibration_accuracy, 3);
    }

    #[test]
    fn parse_uncal_mag() {
        let line = fixture_line(2);
        let r = UncalibratedSensorRecord::parse(&line, "UncalMag").unwrap();

        assert_eq!(r.utc_time_ms, 1771641748239);
        assert!((r.x - (-145.0946)).abs() < 0.001);
        assert!((r.y - (-22.1674)).abs() < 0.001);
        assert!((r.z - (-481.87558)).abs() < 0.001);
        assert!((r.bias_x - (-139.3548)).abs() < 0.001);
        assert_eq!(r.calibration_accuracy, 3);
    }

    #[test]
    fn parse_orientation() {
        let line = fixture_line(3);
        let r = OrientationRecord::parse(&line).unwrap();

        assert_eq!(r.utc_time_ms, 1771641748234);
        assert_eq!(r.elapsed_realtime_ns, 2091904825496862);
        assert!((r.yaw_deg - 24.0).abs() < 0.01);
        assert!((r.roll_deg - (-1.0)).abs() < 0.01);
        assert!((r.pitch_deg - (-40.0)).abs() < 0.01);
        assert_eq!(r.calibration_accuracy, 3);
    }

    #[test]
    fn parse_game_rotation_vector() {
        let line = fixture_line(4);
        let r = GameRotationVectorRecord::parse(&line).unwrap();

        assert_eq!(r.utc_time_ms, 1771641748224);
        assert_eq!(r.elapsed_realtime_ns, 2091904815833060);
        assert!((r.x - 0.25344574).abs() < 1e-7);
        assert!((r.y - 0.24091911).abs() < 1e-7);
        assert!((r.z - 0.6644273).abs() < 1e-6);
        assert!((r.w - 0.6604996).abs() < 1e-6);

        // Verify quaternion is roughly unit-length
        let norm = (r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w).sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn parse_uncal_wrong_field_count() {
        let result = UncalibratedSensorRecord::parse("UncalAccel,123,456", "UncalAccel");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::FieldCount {
                record_type,
                expected,
                actual,
            } => {
                assert_eq!(record_type, "UncalAccel");
                assert_eq!(expected, 10);
                assert_eq!(actual, 3);
            }
            _ => panic!("expected FieldCount error"),
        }
    }

    #[test]
    fn parse_orientation_wrong_field_count() {
        let result = OrientationRecord::parse("OrientationDeg,123");
        assert!(result.is_err());
    }

    #[test]
    fn parse_game_rotation_wrong_field_count() {
        let result = GameRotationVectorRecord::parse("GameRotationVector,123");
        assert!(result.is_err());
    }
}
