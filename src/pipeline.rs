//! Streaming GNSS log processing pipeline.
//!
//! Coordinates all streaming processors (epoch aggregation, dead reckoning,
//! sensor decimation, fix quality classification) into a single pipeline.
//! Platform-independent: no WASM or browser dependencies.
//!
//! # Example
//!
//! ```no_run
//! use std::io::BufRead;
//! use trajix::pipeline::GnssProcessor;
//!
//! let file = std::fs::File::open("gnss_log.txt").unwrap();
//! let reader = std::io::BufReader::new(file);
//! let mut processor = GnssProcessor::new();
//! for line in reader.lines() {
//!     processor.process_line(&line.unwrap());
//! }
//! let result = processor.finalize();
//! ```

use serde::Serialize;

use crate::dead_reckoning::{
    AttitudeSample, DeadReckoning, DeadReckoningConfig, DrDiagnostics, GnssFix, ImuSample,
    SmoothingMethod, TrajectoryPoint, smooth_trajectory,
};
use crate::downsample::{DecimatedSample, StreamingDecimator};
use crate::parser::header::HeaderInfo;
use crate::parser::line::{Record, parse_line};
use crate::parser::time_context::TimestampInferer;
use crate::quality::{FixQuality, FixQualityClassifier};
use crate::record::fix::FixRecord;
use crate::record::status::SatelliteSnapshot;
use crate::summary::{EpochAggregator, FixEpoch, StatusEpoch};

// ────────────────────────────────────────────
// Sensor value types for decimation
// ────────────────────────────────────────────

/// 3-axis sensor value (accelerometer, gyroscope, magnetometer).
#[derive(Debug, Clone, Serialize)]
pub struct SensorXyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Orientation angles in degrees.
#[derive(Debug, Clone, Serialize)]
pub struct OrientationValue {
    pub yaw_deg: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
}

/// Rotation quaternion value.
#[derive(Debug, Clone, Serialize)]
pub struct RotationValue {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

// ────────────────────────────────────────────
// Record counts
// ────────────────────────────────────────────

/// Counts of each record type encountered during processing.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecordCounts {
    pub fix: u64,
    pub status: u64,
    pub raw: u64,
    pub uncal_accel: u64,
    pub uncal_gyro: u64,
    pub uncal_mag: u64,
    pub orientation: u64,
    pub game_rotation: u64,
    pub skipped: u64,
    pub errors: u64,
}

// ────────────────────────────────────────────
// Sensor time series
// ────────────────────────────────────────────

/// Downsampled sensor time-series data (typically 10Hz).
#[derive(Debug, Clone)]
pub struct SensorTimeSeries {
    pub accel: Vec<DecimatedSample<SensorXyz>>,
    pub gyro: Vec<DecimatedSample<SensorXyz>>,
    pub mag: Vec<DecimatedSample<SensorXyz>>,
    pub orientation: Vec<DecimatedSample<OrientationValue>>,
    pub rotation: Vec<DecimatedSample<RotationValue>>,
}

// ────────────────────────────────────────────
// Processing result
// ────────────────────────────────────────────

/// Complete result of processing a GNSS log file.
///
/// Uses native Rust types from the library. For JS serialization,
/// the WASM layer maps these to JS-specific types with `Tsify`.
pub struct ProcessingResult {
    pub header: Option<HeaderInfo>,
    pub lines_parsed: u64,
    pub record_counts: RecordCounts,
    pub fixes: Vec<FixRecord>,
    pub fix_qualities: Vec<FixQuality>,
    pub status_epochs: Vec<StatusEpoch>,
    pub fix_epochs: Vec<FixEpoch>,
    pub dr_trajectory: Vec<TrajectoryPoint>,
    pub dr_diagnostics: DrDiagnostics,
    pub satellite_snapshots: Vec<SatelliteSnapshot>,
    pub sensor_time_series: SensorTimeSeries,
}

// ────────────────────────────────────────────
// GnssProcessor
// ────────────────────────────────────────────

/// Default epoch interval: 1 second.
pub const DEFAULT_EPOCH_MS: i64 = 1000;
/// Default sensor decimation interval: 100ms (100Hz → 10Hz).
pub const SENSOR_DECIMATE_MS: i64 = 100;

/// Streaming GNSS log processor.
///
/// Processes parsed lines one at a time, routing records to the appropriate
/// streaming processors (epoch aggregation, dead reckoning, sensor decimation,
/// fix quality classification).
///
/// This is the platform-independent processing pipeline. For WASM chunk-based
/// buffering, see `trajix-wasm::GnssLogProcessor`.
pub struct GnssProcessor {
    header: Option<HeaderInfo>,
    header_lines: Vec<String>,
    header_done: bool,
    lines_parsed: u64,
    fixes: Vec<FixRecord>,
    fix_qualities: Vec<FixQuality>,
    fix_classifier: FixQualityClassifier,
    aggregator: EpochAggregator,
    dead_reckoning: DeadReckoning,
    accel_decimator: StreamingDecimator<SensorXyz>,
    gyro_decimator: StreamingDecimator<SensorXyz>,
    mag_decimator: StreamingDecimator<SensorXyz>,
    orientation_decimator: StreamingDecimator<OrientationValue>,
    rotation_decimator: StreamingDecimator<RotationValue>,
    satellite_snapshots: Vec<SatelliteSnapshot>,
    timestamp_inferer: TimestampInferer,
    counts: RecordCounts,
}

impl Default for GnssProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl GnssProcessor {
    /// Create a new processor with default configuration.
    pub fn new() -> Self {
        Self::with_config(
            DEFAULT_EPOCH_MS,
            SENSOR_DECIMATE_MS,
            DeadReckoningConfig::default(),
        )
    }

    /// Create a processor with custom configuration.
    pub fn with_config(
        epoch_ms: i64,
        sensor_decimate_ms: i64,
        dr_config: DeadReckoningConfig,
    ) -> Self {
        GnssProcessor {
            header: None,
            header_lines: Vec::new(),
            header_done: false,
            lines_parsed: 0,
            fixes: Vec::new(),
            fix_qualities: Vec::new(),
            fix_classifier: FixQualityClassifier::default(),
            aggregator: EpochAggregator::new(epoch_ms),
            dead_reckoning: DeadReckoning::new(dr_config),
            accel_decimator: StreamingDecimator::new(sensor_decimate_ms),
            gyro_decimator: StreamingDecimator::new(sensor_decimate_ms),
            mag_decimator: StreamingDecimator::new(sensor_decimate_ms),
            orientation_decimator: StreamingDecimator::new(sensor_decimate_ms),
            rotation_decimator: StreamingDecimator::new(sensor_decimate_ms),
            satellite_snapshots: Vec::new(),
            timestamp_inferer: TimestampInferer::new(),
            counts: RecordCounts::default(),
        }
    }

    /// Process a single line of text.
    pub fn process_line(&mut self, line: &str) {
        let line = line.trim();

        // Header lines
        if line.starts_with('#') {
            if !self.header_done {
                self.header_lines.push(line.to_string());
            }
            self.lines_parsed += 1;
            return;
        }

        // First non-comment line: finalize header
        self.finalize_header();

        if line.is_empty() {
            self.lines_parsed += 1;
            return;
        }

        match parse_line(line) {
            None => {}
            Some(Err(_)) => {
                self.counts.errors += 1;
            }
            Some(Ok(Record::Skipped)) => {
                self.counts.skipped += 1;
            }
            Some(Ok(mut record)) => {
                // Time annotation: track timestamps and fill missing Status timestamps
                self.timestamp_inferer.annotate(&mut record);

                match record {
                    Record::Fix(f) => {
                        self.counts.fix += 1;
                        // Feed to aggregator
                        self.aggregator.push(Record::Fix(f.clone()));
                        // Feed to Dead Reckoning
                        self.dead_reckoning.push_gnss(&GnssFix::from(&f));

                        let quality = self.fix_classifier.classify(&f);
                        self.fixes.push(f);
                        self.fix_qualities.push(quality);
                    }
                    Record::Status(s) => {
                        self.counts.status += 1;
                        // Store per-satellite snapshot for sky plot + DuckDB
                        if let Some(ts) = s.unix_time_ms {
                            self.satellite_snapshots
                                .push(SatelliteSnapshot::from_status(&s, ts));
                        }
                        // Feed to aggregator (Status is consumed, not stored)
                        self.aggregator.push(Record::Status(s));
                    }
                    Record::Raw(_) => {
                        self.counts.raw += 1;
                    }
                    Record::UncalAccel(s) => {
                        self.counts.uncal_accel += 1;
                        // Feed to Dead Reckoning (full resolution)
                        self.dead_reckoning.push_imu(&ImuSample::from(&s));
                        // Decimate for chart display (100Hz → 10Hz)
                        let (x, y, z) = s.unbiased();
                        self.accel_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::UncalGyro(s) => {
                        self.counts.uncal_gyro += 1;
                        let (x, y, z) = s.unbiased();
                        self.gyro_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::UncalMag(s) => {
                        self.counts.uncal_mag += 1;
                        let (x, y, z) = s.unbiased();
                        self.mag_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::OrientationDeg(o) => {
                        self.counts.orientation += 1;
                        self.orientation_decimator.push(
                            o.utc_time_ms,
                            OrientationValue {
                                yaw_deg: o.yaw_deg,
                                roll_deg: o.roll_deg,
                                pitch_deg: o.pitch_deg,
                            },
                        );
                    }
                    Record::GameRotationVector(g) => {
                        self.counts.game_rotation += 1;
                        // Feed to Dead Reckoning (full resolution)
                        self.dead_reckoning.push_attitude(&AttitudeSample::from(&g));
                        // Decimate for chart display
                        self.rotation_decimator.push(
                            g.utc_time_ms,
                            RotationValue {
                                x: g.x,
                                y: g.y,
                                z: g.z,
                                w: g.w,
                            },
                        );
                    }
                    Record::Skipped => self.counts.skipped += 1,
                }
            }
        }

        self.lines_parsed += 1;
    }

    /// Finalize processing and collect results.
    ///
    /// Consumes the processor and returns all accumulated results.
    pub fn finalize(mut self) -> ProcessingResult {
        // Finalize header if not done
        self.finalize_header();

        // Finalize streaming processors
        let (status_epochs, fix_epochs) = self.aggregator.finalize();
        let dr_diagnostics = self.dead_reckoning.diagnostics().clone();
        let raw_trajectory = self.dead_reckoning.finalize();
        let dr_trajectory =
            smooth_trajectory(&raw_trajectory, SmoothingMethod::EndpointConstrained);

        ProcessingResult {
            header: self.header,
            lines_parsed: self.lines_parsed,
            record_counts: self.counts,
            fixes: self.fixes,
            fix_qualities: self.fix_qualities,
            status_epochs,
            fix_epochs,
            dr_trajectory,
            dr_diagnostics,
            satellite_snapshots: self.satellite_snapshots,
            sensor_time_series: SensorTimeSeries {
                accel: self.accel_decimator.finalize(),
                gyro: self.gyro_decimator.finalize(),
                mag: self.mag_decimator.finalize(),
                orientation: self.orientation_decimator.finalize(),
                rotation: self.rotation_decimator.finalize(),
            },
        }
    }

    /// Number of lines processed so far.
    pub fn lines_parsed(&self) -> u64 {
        self.lines_parsed
    }

    /// Current record counts.
    pub fn record_counts(&self) -> &RecordCounts {
        &self.counts
    }

    /// Accumulated fix records.
    pub fn fixes(&self) -> &[FixRecord] {
        &self.fixes
    }

    /// Quality tags for each fix (same index as `fixes()`).
    pub fn fix_qualities(&self) -> &[FixQuality] {
        &self.fix_qualities
    }

    /// Per-satellite status snapshots.
    pub fn satellite_snapshots(&self) -> &[SatelliteSnapshot] {
        &self.satellite_snapshots
    }

    /// Parsed header info, if available.
    pub fn header(&self) -> Option<&HeaderInfo> {
        self.header.as_ref()
    }

    fn finalize_header(&mut self) {
        if !self.header_done {
            self.header_done = true;
            let refs: Vec<&str> = self.header_lines.iter().map(|s| s.as_str()).collect();
            self.header = HeaderInfo::parse(&refs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_counts_records() {
        let mut proc = GnssProcessor::new();
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line(
            "Fix,FLP,36.212,140.097,281.3,0.0,3.79,,1771641749000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line("Nav,binary,data,here");

        assert_eq!(proc.record_counts().fix, 2);
        assert_eq!(proc.fixes().len(), 2);
        assert_eq!(proc.fix_qualities().len(), 2);
        assert_eq!(proc.record_counts().skipped, 1);
        // Both GPS and FLP are Primary
        assert_eq!(proc.fix_qualities()[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities()[1], FixQuality::Primary);
    }

    #[test]
    fn nlp_rejected_when_gps_recent() {
        let mut proc = GnssProcessor::new();
        // GPS fix at t=1000
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        // NLP fix 2s later (within 5s gap threshold) — should be Rejected
        proc.process_line("Fix,NLP,36.500,140.500,,,400.0,,1771641750000,,,,,,,,");

        assert_eq!(proc.fixes().len(), 2);
        assert_eq!(proc.fix_qualities()[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities()[1], FixQuality::Rejected);
    }

    #[test]
    fn nlp_gap_fallback_when_no_recent_gps() {
        let mut proc = GnssProcessor::new();
        // GPS fix at t=1000
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        // NLP fix 10s later (beyond 5s gap threshold) — should be GapFallback
        proc.process_line("Fix,NLP,36.500,140.500,,,82.5,,1771641758000,,,,,,,,");

        assert_eq!(proc.fixes().len(), 2);
        assert_eq!(proc.fix_qualities()[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities()[1], FixQuality::GapFallback);
    }

    #[test]
    fn nlp_gap_fallback_when_no_gps_at_all() {
        let mut proc = GnssProcessor::new();
        // NLP fix with no prior GPS/FLP — should be GapFallback
        proc.process_line("Fix,NLP,36.500,140.500,,,82.5,,1771641758000,,,,,,,,");

        assert_eq!(proc.fixes().len(), 1);
        assert_eq!(proc.fix_qualities()[0], FixQuality::GapFallback);
    }

    #[test]
    fn processor_status_time_annotation_and_aggregation() {
        let mut proc = GnssProcessor::new();
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");

        assert_eq!(proc.record_counts().status, 1);
        // Status is consumed by aggregator, not stored in a Vec
    }

    #[test]
    fn processor_sensor_decimation() {
        let mut proc = GnssProcessor::new();

        // Feed 100 accel records at ~100Hz (10ms apart)
        for i in 0..100 {
            let t = 1771641748000i64 + i * 10;
            let line = format!(
                "UncalAccel,{t},2091905471128467,{},{},{},0.0,0.0,0.0,3",
                0.1 * i as f64,
                0.2 * i as f64,
                9.8 + 0.01 * i as f64,
            );
            proc.process_line(&line);
        }

        assert_eq!(proc.record_counts().uncal_accel, 100);

        // Finalize and check decimated output (~10 samples from 100Hz → 10Hz)
        let result = proc.finalize();
        let count = result.sensor_time_series.accel.len();
        assert!(
            (9..=12).contains(&count),
            "expected ~10 decimated samples, got {count}"
        );
    }

    #[test]
    fn processor_dr_runs_inline() {
        let mut proc = GnssProcessor::new();

        // Feed a good Fix (establishes anchor)
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        assert_eq!(proc.record_counts().fix, 1);

        // Feed a degraded Fix (accuracy > 30m, triggers DR)
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,50.0,,1771641750000,,,2091907471128467,30.0,0,,,",
        );
        assert_eq!(proc.record_counts().fix, 2);
    }

    #[test]
    fn processor_aggregator_runs_inline() {
        let mut proc = GnssProcessor::new();

        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");
        proc.process_line("Status,,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0");
        proc.process_line("Fix,GPS,36.212,140.097,281.3,1.5,4.0,,1771641749000,0.82,25.9,2092092474651730,2.8,0,,,");
        proc.process_line(
            "Status,1771641749000,46,0,1,2,1575420030,30.00,192.285,31.194557,1,1,1,22.1",
        );

        assert_eq!(proc.record_counts().fix, 2);
        assert_eq!(proc.record_counts().status, 3);
    }

    // ─── SatelliteSnapshot tests ───

    #[test]
    fn satellite_snapshots_populated_from_status() {
        let mut proc = GnssProcessor::new();
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");
        proc.process_line("Status,,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0");

        assert_eq!(proc.satellite_snapshots().len(), 2);

        // First satellite: GPS, svid=2
        let s0 = &proc.satellite_snapshots()[0];
        assert_eq!(s0.constellation, crate::types::ConstellationType::Gps);
        assert_eq!(s0.svid, 2);
        assert!((s0.azimuth_deg - 192.285).abs() < 0.001);
        assert!((s0.elevation_deg - 31.194557).abs() < 0.001);
        assert!((s0.cn0_dbhz - 25.70).abs() < 0.01);
        assert!(s0.used_in_fix);

        // Second satellite: GLONASS, svid=9
        let s1 = &proc.satellite_snapshots()[1];
        assert_eq!(s1.constellation, crate::types::ConstellationType::Glonass);
        assert_eq!(s1.svid, 9);
        assert!((s1.azimuth_deg - 10.0).abs() < 0.001);
        assert!((s1.elevation_deg - 45.0).abs() < 0.001);
        assert!((s1.cn0_dbhz - 28.40).abs() < 0.01);
        assert!(s1.used_in_fix);
    }

    #[test]
    fn satellite_snapshots_timestamp_inference() {
        let mut proc = GnssProcessor::new();
        proc.process_line(
            "Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,",
        );
        proc.process_line("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");
        proc.process_line("Fix,GPS,36.212,140.097,281.3,1.5,4.0,,1771641749000,0.82,25.9,2092092474651730,2.8,0,,,");
        proc.process_line("Status,,46,0,1,2,1575420030,30.00,180.0,50.0,1,1,1,22.1");

        assert_eq!(proc.satellite_snapshots().len(), 2);
        // First Status: time inferred from Fix at 1771641748000
        assert_eq!(proc.satellite_snapshots()[0].time_ms, 1771641748000);
        // Second Status: time inferred from Fix at 1771641749000
        assert_eq!(proc.satellite_snapshots()[1].time_ms, 1771641749000);
    }

    #[test]
    fn satellite_snapshots_with_explicit_timestamp() {
        let mut proc = GnssProcessor::new();
        proc.process_line("Status,1771641749000,46,0,1,2,1575420030,30.00,180.0,50.0,1,1,1,22.1");

        assert_eq!(proc.satellite_snapshots().len(), 1);
        assert_eq!(proc.satellite_snapshots()[0].time_ms, 1771641749000);
        assert!((proc.satellite_snapshots()[0].azimuth_deg - 180.0).abs() < 0.001);
        assert!((proc.satellite_snapshots()[0].elevation_deg - 50.0).abs() < 0.001);
    }

    #[test]
    fn satellite_snapshots_skipped_without_timestamp() {
        let mut proc = GnssProcessor::new();
        // Status with no timestamp and no preceding record to infer from
        proc.process_line("Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1");

        // No timestamp available → snapshot not stored
        assert_eq!(proc.satellite_snapshots().len(), 0);
        // But status_count still incremented
        assert_eq!(proc.record_counts().status, 1);
    }
}
