use serde::{Deserialize, Serialize};

use crate::error::ParseError;
use crate::types::ConstellationType;

/// Expected number of CSV fields in a Status record (including the "Status" prefix).
const STATUS_FIELD_COUNT: usize = 14;

/// A satellite status record from GNSS Logger.
///
/// 14 CSV fields: Status,UnixTimeMillis,SignalCount,SignalIndex,
/// ConstellationType,Svid,CarrierFrequencyHz,Cn0DbHz,
/// AzimuthDegrees,ElevationDegrees,UsedInFix,HasAlmanacData,
/// HasEphemerisData,BasebandCn0DbHz
///
/// Note: `UnixTimeMillis` is always empty in real data and must be
/// inferred from neighboring records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusRecord {
    /// Always empty in real data; must be inferred from context.
    pub unix_time_ms: Option<i64>,
    pub signal_count: u32,
    pub signal_index: u32,
    pub constellation: ConstellationType,
    pub svid: u32,
    pub carrier_frequency_hz: f64,
    pub cn0_dbhz: f64,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub used_in_fix: bool,
    pub has_almanac_data: bool,
    pub has_ephemeris_data: bool,
    pub baseband_cn0_dbhz: Option<f64>,
}

impl StatusRecord {
    /// Parse a Status record from a CSV line.
    ///
    /// The line should start with "Status," and contain 14 comma-separated fields.
    pub fn parse(line: &str) -> Result<Self, ParseError> {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != STATUS_FIELD_COUNT {
            return Err(ParseError::FieldCount {
                record_type: "Status",
                expected: STATUS_FIELD_COUNT,
                actual: fields.len(),
            });
        }

        let unix_time_ms = parse_optional_i64(fields[1]);

        let constellation_u8 =
            parse_u32(fields[4], "ConstellationType")? as u8;
        let constellation = ConstellationType::from_u8(constellation_u8);

        Ok(StatusRecord {
            unix_time_ms,
            signal_count: parse_u32(fields[2], "SignalCount")?,
            signal_index: parse_u32(fields[3], "SignalIndex")?,
            constellation,
            svid: parse_u32(fields[5], "Svid")?,
            carrier_frequency_hz: parse_f64(fields[6], "CarrierFrequencyHz")?,
            cn0_dbhz: parse_f64(fields[7], "Cn0DbHz")?,
            azimuth_deg: parse_f64(fields[8], "AzimuthDegrees")?,
            elevation_deg: parse_f64(fields[9], "ElevationDegrees")?,
            used_in_fix: fields[10] == "1",
            has_almanac_data: fields[11] == "1",
            has_ephemeris_data: fields[12] == "1",
            baseband_cn0_dbhz: parse_optional_f64(fields[13]),
        })
    }
}

fn parse_f64(s: &str, field: &'static str) -> Result<f64, ParseError> {
    s.parse::<f64>().map_err(|e| ParseError::FieldParse {
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

fn parse_optional_i64(s: &str) -> Option<i64> {
    if s.is_empty() { None } else { s.parse().ok() }
}

fn parse_optional_f64(s: &str) -> Option<f64> {
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
    fn parse_all_status_records() {
        let content = load_fixture("status_multi_constellation.txt");
        let records: Vec<StatusRecord> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| StatusRecord::parse(l).unwrap())
            .collect();

        assert_eq!(records.len(), 46);

        // All records in this epoch report signal_count = 46
        for r in &records {
            assert_eq!(r.signal_count, 46);
        }

        // signal_index should go 0..45
        for (i, r) in records.iter().enumerate() {
            assert_eq!(r.signal_index, i as u32);
        }
    }

    #[test]
    fn unix_time_always_empty() {
        let content = load_fixture("status_multi_constellation.txt");
        for line in content.lines().filter(|l| !l.is_empty()) {
            let r = StatusRecord::parse(line).unwrap();
            assert!(r.unix_time_ms.is_none(), "UnixTimeMillis should be empty");
        }
    }

    #[test]
    fn parse_gps_satellite() {
        let content = load_fixture("status_multi_constellation.txt");
        let line = content.lines().next().unwrap();
        let r = StatusRecord::parse(line).unwrap();

        assert_eq!(r.constellation, ConstellationType::Gps);
        assert_eq!(r.svid, 2);
        assert!((r.carrier_frequency_hz - 1575420030.0).abs() < 1.0);
        assert!((r.cn0_dbhz - 25.70).abs() < 0.01);
        assert!((r.azimuth_deg - 192.285).abs() < 0.001);
        assert!((r.elevation_deg - 31.194557).abs() < 1e-5);
        assert!(r.used_in_fix);
        assert!(r.has_almanac_data);
        assert!(r.has_ephemeris_data);
        assert!((r.baseband_cn0_dbhz.unwrap() - 22.1).abs() < 0.01);
    }

    #[test]
    fn parse_all_constellations() {
        let content = load_fixture("status_multi_constellation.txt");
        let records: Vec<StatusRecord> = content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| StatusRecord::parse(l).unwrap())
            .collect();

        let constellations: std::collections::HashSet<ConstellationType> =
            records.iter().map(|r| r.constellation).collect();

        assert!(constellations.contains(&ConstellationType::Gps));
        assert!(constellations.contains(&ConstellationType::Glonass));
        assert!(constellations.contains(&ConstellationType::Qzss));
        assert!(constellations.contains(&ConstellationType::BeiDou));
        assert!(constellations.contains(&ConstellationType::Galileo));
    }

    #[test]
    fn parse_satellite_not_used_in_fix() {
        let content = load_fixture("status_multi_constellation.txt");
        // Index 14: GLONASS svid 102, not used in fix, no almanac, no ephemeris
        let line = content.lines().nth(14).unwrap();
        let r = StatusRecord::parse(line).unwrap();

        assert_eq!(r.constellation, ConstellationType::Glonass);
        assert_eq!(r.svid, 102);
        assert!(!r.used_in_fix);
        assert!(!r.has_almanac_data);
        assert!(!r.has_ephemeris_data);
    }

    #[test]
    fn parse_beidou_not_used_no_ephemeris() {
        let content = load_fixture("status_multi_constellation.txt");
        // Index 17: BeiDou svid 60, not used in fix, has almanac but no ephemeris
        let line = content.lines().nth(17).unwrap();
        let r = StatusRecord::parse(line).unwrap();

        assert_eq!(r.constellation, ConstellationType::BeiDou);
        assert_eq!(r.svid, 60);
        assert!(!r.used_in_fix);
        assert!(r.has_almanac_data);
        assert!(!r.has_ephemeris_data);
    }

    #[test]
    fn parse_status_wrong_field_count() {
        let result = StatusRecord::parse("Status,,46,0,1,2");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::FieldCount {
                record_type,
                expected,
                actual,
            } => {
                assert_eq!(record_type, "Status");
                assert_eq!(expected, 14);
                assert_eq!(actual, 6);
            }
            _ => panic!("expected FieldCount error"),
        }
    }

    #[test]
    fn parse_status_sbas_constellation() {
        // constellation type 2 is SBAS — should parse without error
        let line = "Status,,46,0,2,100,1575420030,25.0,180.0,45.0,1,1,1,22.0";
        let r = StatusRecord::parse(line).unwrap();
        assert_eq!(r.constellation, ConstellationType::Sbas);
        assert_eq!(r.svid, 100);
    }

    #[test]
    fn parse_status_unknown_constellation() {
        // constellation type 99 is unknown — should still parse
        let line = "Status,,46,0,99,100,1575420030,25.0,180.0,45.0,1,1,1,22.0";
        let r = StatusRecord::parse(line).unwrap();
        assert_eq!(r.constellation, ConstellationType::Unknown(99));
    }
}
