use crate::error::ParseError;
use crate::record::fix::FixRecord;
use crate::record::raw::RawRecord;
use crate::record::sensor::{
    GameRotationVectorRecord, OrientationRecord, UncalibratedSensorRecord,
};
use crate::record::status::StatusRecord;

/// A parsed GNSS Logger record.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Record {
    Fix(FixRecord),
    Status(StatusRecord),
    Raw(RawRecord),
    UncalAccel(UncalibratedSensorRecord),
    UncalGyro(UncalibratedSensorRecord),
    UncalMag(UncalibratedSensorRecord),
    OrientationDeg(OrientationRecord),
    GameRotationVector(GameRotationVectorRecord),
    /// Skipped record types (Nav, Agc, calibrated sensors, etc.)
    Skipped,
}

impl Record {
    /// Extract the Unix timestamp in milliseconds from this record.
    ///
    /// Returns `Some(ms)` for all record types that carry a timestamp.
    /// For Status records, returns the value only if present (it may be empty).
    /// Returns `None` for `Skipped` records and Status records with empty timestamps.
    pub fn timestamp_ms(&self) -> Option<i64> {
        match self {
            Record::Fix(r) => Some(r.unix_time_ms),
            Record::Raw(r) => Some(r.utc_time_ms),
            Record::Status(r) => r.unix_time_ms,
            Record::UncalAccel(r) | Record::UncalGyro(r) | Record::UncalMag(r) => {
                Some(r.utc_time_ms)
            }
            Record::OrientationDeg(r) => Some(r.utc_time_ms),
            Record::GameRotationVector(r) => Some(r.utc_time_ms),
            Record::Skipped => None,
        }
    }

    /// Returns the `RecordType` discriminant for this record.
    ///
    /// Returns `None` for `Skipped` records (which may represent Nav, Agc, etc.).
    pub fn record_type(&self) -> Option<crate::types::RecordType> {
        use crate::types::RecordType;
        match self {
            Record::Fix(_) => Some(RecordType::Fix),
            Record::Status(_) => Some(RecordType::Status),
            Record::Raw(_) => Some(RecordType::Raw),
            Record::UncalAccel(_) => Some(RecordType::UncalAccel),
            Record::UncalGyro(_) => Some(RecordType::UncalGyro),
            Record::UncalMag(_) => Some(RecordType::UncalMag),
            Record::OrientationDeg(_) => Some(RecordType::OrientationDeg),
            Record::GameRotationVector(_) => Some(RecordType::GameRotationVector),
            Record::Skipped => None,
        }
    }
}

/// Record types that are recognized but skipped (no parser needed).
const SKIPPED_PREFIXES: &[&str] = &["Nav,", "Agc,", "Accel,", "Gyro,", "Mag,", "Pressure,"];

/// Parse a single CSV line into a typed Record.
///
/// Returns `None` for comment lines (starting with `#`) and blank lines.
/// Returns `Some(Record::Skipped)` for recognized but unimplemented record types.
pub fn parse_line(line: &str) -> Option<Result<Record, ParseError>> {
    let line = line.trim();

    // Skip blank lines and comments
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    // Dispatch by prefix
    if line.starts_with("Fix,") {
        Some(FixRecord::parse(line).map(Record::Fix))
    } else if line.starts_with("Status,") {
        Some(StatusRecord::parse(line).map(Record::Status))
    } else if line.starts_with("Raw,") {
        Some(RawRecord::parse(line).map(Record::Raw))
    } else if line.starts_with("UncalAccel,") {
        Some(UncalibratedSensorRecord::parse(line, "UncalAccel").map(Record::UncalAccel))
    } else if line.starts_with("UncalGyro,") {
        Some(UncalibratedSensorRecord::parse(line, "UncalGyro").map(Record::UncalGyro))
    } else if line.starts_with("UncalMag,") {
        Some(UncalibratedSensorRecord::parse(line, "UncalMag").map(Record::UncalMag))
    } else if line.starts_with("OrientationDeg,") {
        Some(OrientationRecord::parse(line).map(Record::OrientationDeg))
    } else if line.starts_with("GameRotationVector,") {
        Some(GameRotationVectorRecord::parse(line).map(Record::GameRotationVector))
    } else if SKIPPED_PREFIXES.iter().any(|p| line.starts_with(p)) {
        Some(Ok(Record::Skipped))
    } else {
        // Unknown record type — extract prefix for error message
        let prefix = line.split(',').next().unwrap_or(line);
        Some(Err(ParseError::UnknownRecordType(prefix.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ConstellationType, FixProvider};

    fn load_fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parse_mixed_records() {
        let content = load_fixture("mixed_records.txt");
        let mut counts = RecordCounts::default();

        for line in content.lines() {
            match parse_line(line) {
                None => {} // comment or blank
                Some(Ok(record)) => counts.count(&record),
                Some(Err(e)) => panic!("parse error: {e}"),
            }
        }

        assert_eq!(counts.fix, 2);
        assert_eq!(counts.status, 6);
        assert_eq!(counts.raw, 37);
        assert_eq!(counts.uncal_accel, 12);
        assert_eq!(counts.uncal_gyro, 12);
        assert_eq!(counts.uncal_mag, 7);
        assert_eq!(counts.orientation, 12);
        assert_eq!(counts.game_rotation, 12);
        assert_eq!(counts.skipped, 0);

        // Total should be 95 data lines (some lines in the fixture may be non-data)
        let total = counts.total();
        assert!(total > 0);
    }

    #[test]
    fn parse_fix_via_dispatcher() {
        let line = load_fixture("fix_normal_gps.txt");
        let first = line.lines().next().unwrap();
        let record = parse_line(first).unwrap().unwrap();
        match record {
            Record::Fix(fix) => {
                assert_eq!(fix.provider, FixProvider::Gps);
                assert!(fix.latitude_deg > 36.0);
            }
            _ => panic!("expected Fix record"),
        }
    }

    #[test]
    fn parse_status_via_dispatcher() {
        let line = load_fixture("status_multi_constellation.txt");
        let first = line.lines().next().unwrap();
        let record = parse_line(first).unwrap().unwrap();
        match record {
            Record::Status(s) => {
                assert_eq!(s.constellation, ConstellationType::Gps);
            }
            _ => panic!("expected Status record"),
        }
    }

    #[test]
    fn parse_raw_via_dispatcher() {
        let line = load_fixture("raw_with_ecef.txt");
        let first = line.lines().next().unwrap();
        let record = parse_line(first).unwrap().unwrap();
        match record {
            Record::Raw(r) => {
                assert_eq!(r.constellation, ConstellationType::Gps);
                assert!(r.sv_position_ecef_x_m.is_some());
            }
            _ => panic!("expected Raw record"),
        }
    }

    #[test]
    fn skip_comments_and_blanks() {
        assert!(parse_line("").is_none());
        assert!(parse_line("  ").is_none());
        assert!(parse_line("# comment").is_none());
        assert!(parse_line("# Raw,header,fields").is_none());
    }

    #[test]
    fn skip_nav_and_agc() {
        assert!(matches!(
            parse_line("Nav,1,2,3,4,5,data"),
            Some(Ok(Record::Skipped))
        ));
        assert!(matches!(
            parse_line("Agc,123,456,18,0.0,-1453,0.4,39.6,2.5,13.4,1737,-54.1,1575420030,1"),
            Some(Ok(Record::Skipped))
        ));
    }

    #[test]
    fn unknown_record_type() {
        let result = parse_line("Unknown,field1,field2");
        assert!(matches!(
            result,
            Some(Err(ParseError::UnknownRecordType(_)))
        ));
    }

    #[derive(Default)]
    struct RecordCounts {
        fix: usize,
        status: usize,
        raw: usize,
        uncal_accel: usize,
        uncal_gyro: usize,
        uncal_mag: usize,
        orientation: usize,
        game_rotation: usize,
        skipped: usize,
    }

    impl RecordCounts {
        fn count(&mut self, record: &Record) {
            match record {
                Record::Fix(_) => self.fix += 1,
                Record::Status(_) => self.status += 1,
                Record::Raw(_) => self.raw += 1,
                Record::UncalAccel(_) => self.uncal_accel += 1,
                Record::UncalGyro(_) => self.uncal_gyro += 1,
                Record::UncalMag(_) => self.uncal_mag += 1,
                Record::OrientationDeg(_) => self.orientation += 1,
                Record::GameRotationVector(_) => self.game_rotation += 1,
                Record::Skipped => self.skipped += 1,
            }
        }

        fn total(&self) -> usize {
            self.fix
                + self.status
                + self.raw
                + self.uncal_accel
                + self.uncal_gyro
                + self.uncal_mag
                + self.orientation
                + self.game_rotation
                + self.skipped
        }
    }
}
