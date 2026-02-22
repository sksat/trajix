//! Epoch-level aggregation of GNSS data.
//!
//! Groups Status and Fix records into time bins (epochs) and computes
//! summary statistics: CN0 averages, satellite counts, and accuracy metrics.

use crate::parser::line::Record;
use crate::record::fix::FixRecord;
use crate::record::status::StatusRecord;
use crate::types::ConstellationType;

/// Per-constellation statistics for one time epoch.
#[derive(Debug, Clone)]
pub struct ConstellationStats {
    pub constellation: ConstellationType,
    /// Mean C/N0 (dB-Hz) across all visible satellites of this constellation.
    pub cn0_mean: f64,
    /// Number of visible satellites.
    pub visible: u32,
    /// Number of satellites used in fix.
    pub used_in_fix: u32,
}

/// Aggregated satellite status for one time epoch.
#[derive(Debug, Clone)]
pub struct StatusEpoch {
    /// Epoch start time (milliseconds, Unix epoch).
    pub time_ms: i64,
    /// Mean C/N0 across all visible satellites (dB-Hz).
    pub cn0_mean_all: f64,
    /// Mean C/N0 across satellites used in fix only (dB-Hz).
    /// `f64::NAN` if no satellite was used in fix.
    pub cn0_mean_used: f64,
    /// Total visible satellite count.
    pub num_visible: u32,
    /// Number of satellites used in fix.
    pub num_used: u32,
    /// Per-constellation breakdown, sorted by constellation type ID.
    pub constellations: Vec<ConstellationStats>,
}

/// Fix accuracy summary for one time epoch.
#[derive(Debug, Clone)]
pub struct FixEpoch {
    /// Epoch start time (milliseconds, Unix epoch).
    pub time_ms: i64,
    /// Best (minimum) horizontal accuracy in this epoch (meters).
    pub accuracy_m: Option<f64>,
    /// Best (minimum) vertical accuracy in this epoch (meters).
    pub vertical_accuracy_m: Option<f64>,
    /// Average speed in this epoch (m/s).
    pub speed_mps: Option<f64>,
}

/// Compute the epoch bin start for a given timestamp.
fn epoch_bin(time_ms: i64, interval_ms: i64) -> i64 {
    time_ms.div_euclid(interval_ms) * interval_ms
}

/// Summarize Status records into epoch-level statistics.
///
/// Records are grouped by time bin (`epoch_ms` milliseconds).
/// Records without timestamps are skipped.
///
/// # Panics
/// Panics if `epoch_ms <= 0`.
pub fn summarize_status(records: &[StatusRecord], epoch_ms: i64) -> Vec<StatusEpoch> {
    assert!(epoch_ms > 0, "epoch_ms must be positive");

    let mut epochs = Vec::new();
    let mut current_bin = i64::MIN;
    let mut bin_records: Vec<&StatusRecord> = Vec::new();

    for record in records {
        let ts = match record.unix_time_ms {
            Some(t) => t,
            None => continue,
        };
        let bin = epoch_bin(ts, epoch_ms);

        if bin != current_bin {
            if !bin_records.is_empty() {
                epochs.push(compute_status_epoch(current_bin, &bin_records));
            }
            current_bin = bin;
            bin_records.clear();
        }
        bin_records.push(record);
    }

    if !bin_records.is_empty() {
        epochs.push(compute_status_epoch(current_bin, &bin_records));
    }

    epochs
}

fn compute_status_epoch(time_ms: i64, records: &[&StatusRecord]) -> StatusEpoch {
    let num_visible = records.len() as u32;
    let num_used = records.iter().filter(|r| r.used_in_fix).count() as u32;

    let cn0_mean_all = records.iter().map(|r| r.cn0_dbhz).sum::<f64>() / num_visible as f64;

    let cn0_mean_used = if num_used > 0 {
        records
            .iter()
            .filter(|r| r.used_in_fix)
            .map(|r| r.cn0_dbhz)
            .sum::<f64>()
            / num_used as f64
    } else {
        f64::NAN
    };

    // Per-constellation: BTreeMap for deterministic ordering by constellation ID
    let mut by_constellation: std::collections::BTreeMap<u8, (ConstellationType, f64, u32, u32)> =
        std::collections::BTreeMap::new();

    for r in records {
        let key = r.constellation.as_u8();
        let entry = by_constellation
            .entry(key)
            .or_insert((r.constellation, 0.0, 0, 0));
        entry.1 += r.cn0_dbhz;
        entry.2 += 1;
        if r.used_in_fix {
            entry.3 += 1;
        }
    }

    let constellations = by_constellation
        .values()
        .map(
            |&(constellation, cn0_sum, visible, used)| ConstellationStats {
                constellation,
                cn0_mean: cn0_sum / visible as f64,
                visible,
                used_in_fix: used,
            },
        )
        .collect();

    StatusEpoch {
        time_ms,
        cn0_mean_all,
        cn0_mean_used,
        num_visible,
        num_used,
        constellations,
    }
}

/// Summarize Fix records into epoch-level accuracy statistics.
///
/// For accuracy, takes the best (minimum) value in each epoch.
/// For speed, takes the average.
///
/// # Panics
/// Panics if `epoch_ms <= 0`.
pub fn summarize_fixes(records: &[FixRecord], epoch_ms: i64) -> Vec<FixEpoch> {
    assert!(epoch_ms > 0, "epoch_ms must be positive");

    let mut epochs = Vec::new();
    let mut current_bin = i64::MIN;
    let mut bin_records: Vec<&FixRecord> = Vec::new();

    for record in records {
        let bin = epoch_bin(record.unix_time_ms, epoch_ms);

        if bin != current_bin {
            if !bin_records.is_empty() {
                epochs.push(compute_fix_epoch(current_bin, &bin_records));
            }
            current_bin = bin;
            bin_records.clear();
        }
        bin_records.push(record);
    }

    if !bin_records.is_empty() {
        epochs.push(compute_fix_epoch(current_bin, &bin_records));
    }

    epochs
}

fn compute_fix_epoch(time_ms: i64, records: &[&FixRecord]) -> FixEpoch {
    let accuracy_m = records
        .iter()
        .filter_map(|r| r.accuracy_m)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let vertical_accuracy_m = records
        .iter()
        .filter_map(|r| r.vertical_accuracy_m)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let speeds: Vec<f64> = records.iter().filter_map(|r| r.speed_mps).collect();
    let speed_mps = if speeds.is_empty() {
        None
    } else {
        Some(speeds.iter().sum::<f64>() / speeds.len() as f64)
    };

    FixEpoch {
        time_ms,
        accuracy_m,
        vertical_accuracy_m,
        speed_mps,
    }
}

/// Streaming epoch aggregator.
///
/// Accepts `Record` values one at a time and accumulates epoch-level
/// summaries. Records should arrive in roughly chronological order;
/// when a record falls into a new epoch bin, the previous bin is flushed.
pub struct EpochAggregator {
    epoch_ms: i64,
    status_bin: Option<i64>,
    status_buf: Vec<StatusRecord>,
    fix_bin: Option<i64>,
    fix_buf: Vec<FixRecord>,
    status_epochs: Vec<StatusEpoch>,
    fix_epochs: Vec<FixEpoch>,
}

impl EpochAggregator {
    pub fn new(epoch_ms: i64) -> Self {
        assert!(epoch_ms > 0, "epoch_ms must be positive");
        EpochAggregator {
            epoch_ms,
            status_bin: None,
            status_buf: Vec::new(),
            fix_bin: None,
            fix_buf: Vec::new(),
            status_epochs: Vec::new(),
            fix_epochs: Vec::new(),
        }
    }

    /// Feed a record into the aggregator.
    ///
    /// Only Status and Fix records are accumulated; other types are ignored.
    pub fn push(&mut self, record: Record) {
        match record {
            Record::Status(s) => {
                if let Some(ts) = s.unix_time_ms {
                    let bin = epoch_bin(ts, self.epoch_ms);
                    if self.status_bin != Some(bin) {
                        self.flush_status();
                        self.status_bin = Some(bin);
                    }
                    self.status_buf.push(s);
                }
            }
            Record::Fix(f) => {
                let bin = epoch_bin(f.unix_time_ms, self.epoch_ms);
                if self.fix_bin != Some(bin) {
                    self.flush_fix();
                    self.fix_bin = Some(bin);
                }
                self.fix_buf.push(f);
            }
            _ => {}
        }
    }

    /// Flush remaining buffers and return all accumulated epochs.
    pub fn finalize(mut self) -> (Vec<StatusEpoch>, Vec<FixEpoch>) {
        self.flush_status();
        self.flush_fix();
        (self.status_epochs, self.fix_epochs)
    }

    fn flush_status(&mut self) {
        if let Some(bin) = self.status_bin
            && !self.status_buf.is_empty()
        {
            let refs: Vec<&StatusRecord> = self.status_buf.iter().collect();
            self.status_epochs.push(compute_status_epoch(bin, &refs));
            self.status_buf.clear();
        }
    }

    fn flush_fix(&mut self) {
        if let Some(bin) = self.fix_bin
            && !self.fix_buf.is_empty()
        {
            let refs: Vec<&FixRecord> = self.fix_buf.iter().collect();
            self.fix_epochs.push(compute_fix_epoch(bin, &refs));
            self.fix_buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FixProvider;

    fn make_status(
        time_ms: i64,
        constellation: ConstellationType,
        svid: u32,
        cn0: f64,
        used: bool,
    ) -> StatusRecord {
        StatusRecord {
            unix_time_ms: Some(time_ms),
            signal_count: 0,
            signal_index: 0,
            constellation,
            svid,
            carrier_frequency_hz: 1575420030.0,
            cn0_dbhz: cn0,
            azimuth_deg: 180.0,
            elevation_deg: 45.0,
            used_in_fix: used,
            has_almanac_data: true,
            has_ephemeris_data: true,
            baseband_cn0_dbhz: None,
        }
    }

    fn make_fix(
        time_ms: i64,
        accuracy: Option<f64>,
        vert_accuracy: Option<f64>,
        speed: Option<f64>,
    ) -> FixRecord {
        FixRecord {
            provider: FixProvider::Gps,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
            altitude_m: Some(100.0),
            speed_mps: speed,
            accuracy_m: accuracy,
            bearing_deg: None,
            unix_time_ms: time_ms,
            speed_accuracy_mps: None,
            bearing_accuracy_deg: None,
            elapsed_realtime_ns: None,
            vertical_accuracy_m: vert_accuracy,
            mock_location: false,
            num_used_signals: None,
            vertical_speed_accuracy_mps: None,
            solution_type: None,
        }
    }

    // ──────────────────────────────────────────────
    // epoch_bin
    // ──────────────────────────────────────────────

    #[test]
    fn epoch_bin_basics() {
        assert_eq!(epoch_bin(1500, 1000), 1000);
        assert_eq!(epoch_bin(2000, 1000), 2000);
        assert_eq!(epoch_bin(0, 1000), 0);
        assert_eq!(epoch_bin(999, 1000), 0);
        assert_eq!(epoch_bin(1000, 1000), 1000);
    }

    // ──────────────────────────────────────────────
    // summarize_status
    // ──────────────────────────────────────────────

    #[test]
    fn status_single_epoch() {
        let records = vec![
            make_status(1000, ConstellationType::Gps, 1, 30.0, true),
            make_status(1100, ConstellationType::Gps, 2, 20.0, true),
            make_status(1200, ConstellationType::Gps, 3, 10.0, false),
        ];
        let epochs = summarize_status(&records, 1000);
        assert_eq!(epochs.len(), 1);

        let e = &epochs[0];
        assert_eq!(e.time_ms, 1000);
        assert_eq!(e.num_visible, 3);
        assert_eq!(e.num_used, 2);
        assert!((e.cn0_mean_all - 20.0).abs() < 1e-10); // (30+20+10)/3
        assert!((e.cn0_mean_used - 25.0).abs() < 1e-10); // (30+20)/2
    }

    #[test]
    fn status_two_epochs() {
        let records = vec![
            make_status(1000, ConstellationType::Gps, 1, 30.0, true),
            make_status(1100, ConstellationType::Gps, 2, 20.0, true),
            // New epoch
            make_status(2000, ConstellationType::Gps, 1, 35.0, true),
            make_status(2100, ConstellationType::Gps, 2, 25.0, false),
        ];
        let epochs = summarize_status(&records, 1000);
        assert_eq!(epochs.len(), 2);

        assert_eq!(epochs[0].time_ms, 1000);
        assert_eq!(epochs[0].num_visible, 2);
        assert_eq!(epochs[0].num_used, 2);

        assert_eq!(epochs[1].time_ms, 2000);
        assert_eq!(epochs[1].num_visible, 2);
        assert_eq!(epochs[1].num_used, 1);
        assert!((epochs[1].cn0_mean_used - 35.0).abs() < 1e-10);
    }

    #[test]
    fn status_multi_constellation() {
        let records = vec![
            make_status(1000, ConstellationType::Gps, 1, 30.0, true),
            make_status(1000, ConstellationType::Gps, 2, 20.0, false),
            make_status(1000, ConstellationType::Galileo, 1, 35.0, true),
            make_status(1000, ConstellationType::Glonass, 5, 15.0, false),
        ];
        let epochs = summarize_status(&records, 1000);
        assert_eq!(epochs.len(), 1);

        let e = &epochs[0];
        assert_eq!(e.num_visible, 4);
        assert_eq!(e.num_used, 2);
        assert_eq!(e.constellations.len(), 3);

        // Sorted by constellation ID: GPS(1), Glonass(3), Galileo(6)
        let gps = &e.constellations[0];
        assert_eq!(gps.constellation, ConstellationType::Gps);
        assert_eq!(gps.visible, 2);
        assert_eq!(gps.used_in_fix, 1);
        assert!((gps.cn0_mean - 25.0).abs() < 1e-10);

        let glonass = &e.constellations[1];
        assert_eq!(glonass.constellation, ConstellationType::Glonass);
        assert_eq!(glonass.visible, 1);
        assert_eq!(glonass.used_in_fix, 0);

        let galileo = &e.constellations[2];
        assert_eq!(galileo.constellation, ConstellationType::Galileo);
        assert_eq!(galileo.visible, 1);
        assert_eq!(galileo.used_in_fix, 1);
    }

    #[test]
    fn status_no_used_satellites() {
        let records = vec![
            make_status(1000, ConstellationType::Gps, 1, 10.0, false),
            make_status(1000, ConstellationType::Gps, 2, 15.0, false),
        ];
        let epochs = summarize_status(&records, 1000);
        assert_eq!(epochs.len(), 1);
        assert_eq!(epochs[0].num_used, 0);
        assert!(epochs[0].cn0_mean_used.is_nan());
    }

    #[test]
    fn status_skips_no_timestamp() {
        let mut records = vec![make_status(1000, ConstellationType::Gps, 1, 30.0, true)];
        records.push(StatusRecord {
            unix_time_ms: None,
            signal_count: 0,
            signal_index: 0,
            constellation: ConstellationType::Gps,
            svid: 99,
            carrier_frequency_hz: 0.0,
            cn0_dbhz: 50.0,
            azimuth_deg: 0.0,
            elevation_deg: 0.0,
            used_in_fix: true,
            has_almanac_data: false,
            has_ephemeris_data: false,
            baseband_cn0_dbhz: None,
        });

        let epochs = summarize_status(&records, 1000);
        assert_eq!(epochs.len(), 1);
        assert_eq!(epochs[0].num_visible, 1); // Only the one with timestamp
    }

    #[test]
    fn status_empty_input() {
        let epochs = summarize_status(&[], 1000);
        assert!(epochs.is_empty());
    }

    // ──────────────────────────────────────────────
    // summarize_fixes
    // ──────────────────────────────────────────────

    #[test]
    fn fix_single_epoch() {
        let records = vec![make_fix(1500, Some(5.0), Some(3.0), Some(10.0))];
        let epochs = summarize_fixes(&records, 1000);
        assert_eq!(epochs.len(), 1);

        let e = &epochs[0];
        assert_eq!(e.time_ms, 1000);
        assert!((e.accuracy_m.unwrap() - 5.0).abs() < 1e-10);
        assert!((e.vertical_accuracy_m.unwrap() - 3.0).abs() < 1e-10);
        assert!((e.speed_mps.unwrap() - 10.0).abs() < 1e-10);
    }

    #[test]
    fn fix_best_accuracy_in_epoch() {
        let records = vec![
            make_fix(1000, Some(10.0), Some(8.0), Some(5.0)),
            make_fix(1100, Some(5.0), Some(3.0), Some(15.0)),
            make_fix(1200, Some(8.0), Some(6.0), Some(10.0)),
        ];
        let epochs = summarize_fixes(&records, 1000);
        assert_eq!(epochs.len(), 1);

        let e = &epochs[0];
        assert!((e.accuracy_m.unwrap() - 5.0).abs() < 1e-10); // min
        assert!((e.vertical_accuracy_m.unwrap() - 3.0).abs() < 1e-10); // min
        assert!((e.speed_mps.unwrap() - 10.0).abs() < 1e-10); // (5+15+10)/3
    }

    #[test]
    fn fix_two_epochs() {
        let records = vec![
            make_fix(1000, Some(5.0), Some(3.0), Some(10.0)),
            make_fix(2500, Some(8.0), Some(4.0), Some(20.0)),
        ];
        let epochs = summarize_fixes(&records, 1000);
        assert_eq!(epochs.len(), 2);

        assert_eq!(epochs[0].time_ms, 1000);
        assert!((epochs[0].accuracy_m.unwrap() - 5.0).abs() < 1e-10);
        assert_eq!(epochs[1].time_ms, 2000);
        assert!((epochs[1].accuracy_m.unwrap() - 8.0).abs() < 1e-10);
    }

    #[test]
    fn fix_none_fields() {
        let records = vec![make_fix(1000, None, None, None)];
        let epochs = summarize_fixes(&records, 1000);
        assert_eq!(epochs.len(), 1);

        let e = &epochs[0];
        assert!(e.accuracy_m.is_none());
        assert!(e.vertical_accuracy_m.is_none());
        assert!(e.speed_mps.is_none());
    }

    #[test]
    fn fix_empty_input() {
        let epochs = summarize_fixes(&[], 1000);
        assert!(epochs.is_empty());
    }

    // ──────────────────────────────────────────────
    // EpochAggregator
    // ──────────────────────────────────────────────

    #[test]
    fn aggregator_basic() {
        let mut agg = EpochAggregator::new(1000);

        agg.push(Record::Status(make_status(
            1000,
            ConstellationType::Gps,
            1,
            30.0,
            true,
        )));
        agg.push(Record::Status(make_status(
            1100,
            ConstellationType::Gps,
            2,
            20.0,
            false,
        )));
        agg.push(Record::Fix(make_fix(
            1050,
            Some(5.0),
            Some(3.0),
            Some(10.0),
        )));

        // New epoch
        agg.push(Record::Status(make_status(
            2000,
            ConstellationType::Gps,
            1,
            35.0,
            true,
        )));
        agg.push(Record::Fix(make_fix(
            2050,
            Some(8.0),
            Some(4.0),
            Some(20.0),
        )));

        let (status, fix) = agg.finalize();
        assert_eq!(status.len(), 2);
        assert_eq!(fix.len(), 2);

        assert_eq!(status[0].time_ms, 1000);
        assert_eq!(status[0].num_visible, 2);
        assert_eq!(status[1].time_ms, 2000);
        assert_eq!(status[1].num_visible, 1);

        assert_eq!(fix[0].time_ms, 1000);
        assert!((fix[0].accuracy_m.unwrap() - 5.0).abs() < 1e-10);
        assert_eq!(fix[1].time_ms, 2000);
    }

    #[test]
    fn aggregator_ignores_other_records() {
        let mut agg = EpochAggregator::new(1000);
        agg.push(Record::Skipped);

        let (status, fix) = agg.finalize();
        assert!(status.is_empty());
        assert!(fix.is_empty());
    }

    #[test]
    fn aggregator_with_parsed_lines() {
        use crate::parser::line::parse_line;

        let input = "\
Fix,GPS,36.212,140.097,284.0,0.0,5.0,,1771641935000,,,2092092474651730,3.0,0,,,
Status,1771641935000,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Status,1771641935000,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0
Fix,GPS,36.212,140.097,284.0,1.5,4.0,,1771641936000,,,2092092474651730,2.5,0,,,
Status,1771641936000,46,0,1,2,1575420030,30.00,192.285,31.194557,1,1,1,22.1";

        let mut agg = EpochAggregator::new(1000);
        for line in input.lines() {
            if let Some(Ok(record)) = parse_line(line) {
                agg.push(record);
            }
        }

        let (status, fix) = agg.finalize();

        // Status: 2 at 1771641935000, 1 at 1771641936000 → different 1s bins
        assert_eq!(status.len(), 2);
        assert_eq!(status[0].num_visible, 2);
        assert_eq!(status[1].num_visible, 1);

        // Fix: 1 at 1771641935000, 1 at 1771641936000
        assert_eq!(fix.len(), 2);
        assert!((fix[0].accuracy_m.unwrap() - 5.0).abs() < 1e-10);
        assert!((fix[1].accuracy_m.unwrap() - 4.0).abs() < 1e-10);
    }
}
