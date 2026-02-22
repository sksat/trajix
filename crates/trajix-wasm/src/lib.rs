use serde::Serialize;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use trajix::dead_reckoning::{DeadReckoning, DrConfig, DrSmoothing, DrSource, smooth_trajectory};
use trajix::downsample::{DecimatedSample, StreamingDecimator};
use trajix::parser::header::HeaderInfo;
use trajix::parser::line::{Record, parse_line};
use trajix::parser::time_context::TimestampInferer;
use trajix::quality::{FixQuality, FixQualityClassifier};
use trajix::record::fix::FixRecord;
use trajix::record::status::SatelliteSnapshot;
use trajix::summary::EpochAggregator;

// ────────────────────────────────────────────
// Sensor value types for decimation
// ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Tsify)]
struct SensorXyz {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Clone, Serialize, Tsify)]
struct OrientationValue {
    yaw_deg: f64,
    roll_deg: f64,
    pitch_deg: f64,
}

#[derive(Debug, Clone, Serialize, Tsify)]
struct RotationValue {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

// ────────────────────────────────────────────
// GnssLogProcessor: chunk-based streaming parser
// ────────────────────────────────────────────

/// Streaming GNSS log processor for browser use.
///
/// Parses GNSS Logger data incrementally via `feed(chunk)`.
/// Runs epoch aggregation, Dead Reckoning, and sensor downsampling
/// inline during parsing — only lightweight results are returned.
///
/// Usage from JS:
/// ```js
/// const processor = new GnssLogProcessor();
/// for (const chunk of chunks) {
///   processor.feed(chunk);
/// }
/// const result = processor.finalize();
/// ```
#[wasm_bindgen]
pub struct GnssLogProcessor {
    // ─── Chunk handling ───
    remainder: Vec<u8>,
    lines_parsed: u64,
    bytes_fed: u64,

    // ─── Header ───
    header: Option<HeaderInfo>,
    header_lines: Vec<String>,
    header_done: bool,

    // ─── Fix records (kept in full — only ~52K, ~8MB) ───
    fixes: Vec<FixRecord>,
    /// Parallel quality tag for each fix (same index as `fixes`).
    fix_qualities: Vec<FixQuality>,
    /// Streaming fix quality classifier.
    fix_classifier: FixQualityClassifier,

    // ─── Streaming processors (consume records, don't store them) ───
    aggregator: EpochAggregator,
    dead_reckoning: DeadReckoning,

    // ─── Sensor decimators (100Hz → 10Hz for time-series charts) ───
    accel_decimator: StreamingDecimator<SensorXyz>,
    gyro_decimator: StreamingDecimator<SensorXyz>,
    mag_decimator: StreamingDecimator<SensorXyz>,
    orientation_decimator: StreamingDecimator<OrientationValue>,
    rotation_decimator: StreamingDecimator<RotationValue>,

    // ─── Per-satellite snapshots (for sky plot + DuckDB status table) ───
    satellite_snapshots: Vec<SatelliteSnapshot>,

    // ─── Time context for Status timestamp inference ───
    timestamp_inferer: TimestampInferer,

    // ─── Counts ───
    fix_count: u64,
    status_count: u64,
    raw_count: u64,
    uncal_accel_count: u64,
    uncal_gyro_count: u64,
    uncal_mag_count: u64,
    orientation_count: u64,
    game_rotation_count: u64,
    skipped_count: u64,
    error_count: u64,
}

/// Default epoch interval: 1 second.
const DEFAULT_EPOCH_MS: i64 = 1000;
/// Default sensor decimation interval: 100ms (100Hz → 10Hz).
const SENSOR_DECIMATE_MS: i64 = 100;

impl Default for GnssLogProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl GnssLogProcessor {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        GnssLogProcessor {
            remainder: Vec::new(),
            lines_parsed: 0,
            bytes_fed: 0,
            header: None,
            header_lines: Vec::new(),
            header_done: false,
            fixes: Vec::new(),
            fix_qualities: Vec::new(),
            fix_classifier: FixQualityClassifier::default(),
            aggregator: EpochAggregator::new(DEFAULT_EPOCH_MS),
            dead_reckoning: DeadReckoning::new(DrConfig::default()),
            accel_decimator: StreamingDecimator::new(SENSOR_DECIMATE_MS),
            gyro_decimator: StreamingDecimator::new(SENSOR_DECIMATE_MS),
            mag_decimator: StreamingDecimator::new(SENSOR_DECIMATE_MS),
            orientation_decimator: StreamingDecimator::new(SENSOR_DECIMATE_MS),
            rotation_decimator: StreamingDecimator::new(SENSOR_DECIMATE_MS),
            satellite_snapshots: Vec::new(),
            timestamp_inferer: TimestampInferer::new(),
            fix_count: 0,
            status_count: 0,
            raw_count: 0,
            uncal_accel_count: 0,
            uncal_gyro_count: 0,
            uncal_mag_count: 0,
            orientation_count: 0,
            game_rotation_count: 0,
            skipped_count: 0,
            error_count: 0,
        }
    }

    /// Feed a chunk of bytes (UTF-8 text) into the processor.
    ///
    /// Returns the number of lines parsed from this chunk.
    /// Handles chunk boundaries: incomplete lines are buffered until the next chunk.
    pub fn feed(&mut self, chunk: &[u8]) -> u64 {
        self.bytes_fed += chunk.len() as u64;

        // Prepend remainder from previous chunk
        let data = if self.remainder.is_empty() {
            chunk.to_vec()
        } else {
            let mut buf = std::mem::take(&mut self.remainder);
            buf.extend_from_slice(chunk);
            buf
        };

        let mut lines_this_chunk = 0u64;
        let mut start = 0;

        for (i, &byte) in data.iter().enumerate() {
            if byte == b'\n' {
                let line_bytes = &data[start..i];
                let line = String::from_utf8_lossy(line_bytes);
                let line = line.trim_end_matches('\r');
                self.process_line(line);
                lines_this_chunk += 1;
                start = i + 1;
            }
        }

        // Save remainder (incomplete line at end of chunk)
        if start < data.len() {
            self.remainder = data[start..].to_vec();
        }

        self.lines_parsed += lines_this_chunk;
        lines_this_chunk
    }

    /// Finalize processing and return lightweight results.
    ///
    /// Returns: Fix records, epoch summaries, DR trajectory,
    /// downsampled sensor time-series, and record counts.
    /// Total size is typically ~30-50 MB (vs ~3 GB if all records were returned).
    pub fn finalize(mut self) -> Result<JsValue, JsValue> {
        // Process any remaining data in the buffer
        if !self.remainder.is_empty() {
            let remaining = std::mem::take(&mut self.remainder);
            let line = String::from_utf8_lossy(&remaining);
            let line = line.trim_end_matches('\r');
            if !line.is_empty() {
                self.process_line(line);
                self.lines_parsed += 1;
            }
        }

        // Finalize header if not done
        self.finalize_header();

        // Finalize streaming processors
        let (status_epochs, fix_epochs) = self.aggregator.finalize();
        let raw_trajectory = self.dead_reckoning.finalize();
        let dr_trajectory =
            smooth_trajectory(&raw_trajectory, DrSmoothing::EndpointConstrained);

        let result = ProcessingResult {
            header: self.header,
            lines_parsed: self.lines_parsed,
            record_counts: RecordCounts {
                fix: self.fix_count,
                status: self.status_count,
                raw: self.raw_count,
                uncal_accel: self.uncal_accel_count,
                uncal_gyro: self.uncal_gyro_count,
                uncal_mag: self.uncal_mag_count,
                orientation: self.orientation_count,
                game_rotation: self.game_rotation_count,
                skipped: self.skipped_count,
                errors: self.error_count,
            },

            // Fix records (full resolution, ~52K)
            fixes: self.fixes,
            fix_qualities: self.fix_qualities,

            // Epoch summaries (aggregated from Status + Fix)
            status_epochs: status_epochs
                .into_iter()
                .map(|e| StatusEpochJs {
                    time_ms: e.time_ms,
                    cn0_mean_all: e.cn0_mean_all,
                    cn0_mean_used: e.cn0_mean_used,
                    num_visible: e.num_visible,
                    num_used: e.num_used,
                    constellations: e
                        .constellations
                        .into_iter()
                        .map(|c| ConstellationStatsJs {
                            constellation: c.constellation.as_u8(),
                            constellation_name: c.constellation.name().to_string(),
                            cn0_mean: c.cn0_mean,
                            visible: c.visible,
                            used_in_fix: c.used_in_fix,
                        })
                        .collect(),
                })
                .collect(),

            fix_epochs: fix_epochs
                .into_iter()
                .map(|e| FixEpochJs {
                    time_ms: e.time_ms,
                    accuracy_m: e.accuracy_m,
                    vertical_accuracy_m: e.vertical_accuracy_m,
                    speed_mps: e.speed_mps,
                })
                .collect(),

            // Dead Reckoning trajectory
            dr_trajectory: dr_trajectory
                .into_iter()
                .map(|p| DrPointJs {
                    time_ms: p.time_ms,
                    latitude_deg: p.latitude_deg,
                    longitude_deg: p.longitude_deg,
                    altitude_m: p.altitude_m,
                    source: match p.source {
                        DrSource::Gnss => "gnss",
                        DrSource::DeadReckoning => "dr",
                    },
                })
                .collect(),

            // Per-satellite snapshots (for sky plot + DuckDB)
            satellite_snapshots: self.satellite_snapshots,

            // Downsampled sensor time-series (10Hz)
            sensor_time_series: SensorTimeSeries {
                accel: self.accel_decimator.finalize(),
                gyro: self.gyro_decimator.finalize(),
                mag: self.mag_decimator.finalize(),
                orientation: self.orientation_decimator.finalize(),
                rotation: self.rotation_decimator.finalize(),
            },
        };

        serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Number of lines parsed so far.
    pub fn lines_parsed(&self) -> u64 {
        self.lines_parsed
    }

    /// Total bytes fed so far.
    pub fn bytes_fed(&self) -> u64 {
        self.bytes_fed
    }

    /// Progress ratio (0.0 to 1.0) given total file size.
    pub fn progress(&self, total_bytes: u64) -> f64 {
        if total_bytes == 0 {
            return 0.0;
        }
        (self.bytes_fed as f64 / total_bytes as f64).min(1.0)
    }
}

// Non-wasm_bindgen methods
impl GnssLogProcessor {
    fn process_line(&mut self, line: &str) {
        let line = line.trim();

        // Header lines
        if line.starts_with('#') {
            if !self.header_done {
                self.header_lines.push(line.to_string());
            }
            return;
        }

        // First non-comment line: finalize header
        self.finalize_header();

        if line.is_empty() {
            return;
        }

        match parse_line(line) {
            None => {}
            Some(Err(_)) => {
                self.error_count += 1;
            }
            Some(Ok(Record::Skipped)) => {
                self.skipped_count += 1;
            }
            Some(Ok(mut record)) => {
                // Time annotation: track timestamps and fill missing Status timestamps
                self.timestamp_inferer.annotate(&mut record);

                match record {
                    Record::Fix(f) => {
                        self.fix_count += 1;
                        // Feed to aggregator
                        self.aggregator.push(Record::Fix(f.clone()));
                        // Feed to Dead Reckoning
                        self.dead_reckoning.push_fix(&f);

                        let quality = self.fix_classifier.classify(&f);
                        self.fixes.push(f);
                        self.fix_qualities.push(quality);
                    }
                    Record::Status(s) => {
                        self.status_count += 1;
                        // Store per-satellite snapshot for sky plot + DuckDB
                        if let Some(ts) = s.unix_time_ms {
                            self.satellite_snapshots
                                .push(SatelliteSnapshot::from_status(&s, ts));
                        }
                        // Feed to aggregator (Status is consumed, not stored)
                        self.aggregator.push(Record::Status(s));
                    }
                    Record::Raw(_) => {
                        self.raw_count += 1;
                        // Raw records: counted only for now.
                        // Future: emit as CSV for DuckDB ingestion.
                    }
                    Record::UncalAccel(s) => {
                        self.uncal_accel_count += 1;
                        // Feed to Dead Reckoning (full resolution)
                        self.dead_reckoning.push_accel(&s);
                        // Decimate for chart display (100Hz → 10Hz)
                        let (x, y, z) = s.unbiased();
                        self.accel_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::UncalGyro(s) => {
                        self.uncal_gyro_count += 1;
                        let (x, y, z) = s.unbiased();
                        self.gyro_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::UncalMag(s) => {
                        self.uncal_mag_count += 1;
                        let (x, y, z) = s.unbiased();
                        self.mag_decimator
                            .push(s.utc_time_ms, SensorXyz { x, y, z });
                    }
                    Record::OrientationDeg(o) => {
                        self.orientation_count += 1;
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
                        self.game_rotation_count += 1;
                        // Feed to Dead Reckoning (full resolution)
                        self.dead_reckoning.push_attitude(&g);
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
                    Record::Skipped => self.skipped_count += 1,
                }
            }
        }
    }

    fn finalize_header(&mut self) {
        if !self.header_done {
            self.header_done = true;
            let refs: Vec<&str> = self.header_lines.iter().map(|s| s.as_str()).collect();
            self.header = HeaderInfo::parse(&refs);
        }
    }
}

// ────────────────────────────────────────────
// Result types (serialized to JS via serde-wasm-bindgen)
// ────────────────────────────────────────────

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
struct ProcessingResult {
    header: Option<HeaderInfo>,
    lines_parsed: u64,
    record_counts: RecordCounts,

    /// Fix records (full resolution, ~52K records for map rendering).
    fixes: Vec<FixRecord>,
    /// Parallel quality tags (same index as `fixes`).
    fix_qualities: Vec<FixQuality>,

    /// Epoch-aggregated satellite status (1-second bins).
    status_epochs: Vec<StatusEpochJs>,
    /// Epoch-aggregated fix accuracy (1-second bins).
    fix_epochs: Vec<FixEpochJs>,

    /// Dead Reckoning trajectory (GNSS + IMU fusion).
    dr_trajectory: Vec<DrPointJs>,

    /// Per-satellite status snapshots for sky plot and DuckDB status table.
    satellite_snapshots: Vec<SatelliteSnapshot>,

    /// Downsampled sensor data (10Hz) for time-series charts.
    sensor_time_series: SensorTimeSeries,
}

#[derive(Serialize, Tsify)]
struct RecordCounts {
    fix: u64,
    status: u64,
    raw: u64,
    uncal_accel: u64,
    uncal_gyro: u64,
    uncal_mag: u64,
    orientation: u64,
    game_rotation: u64,
    skipped: u64,
    errors: u64,
}

#[derive(Serialize, Tsify)]
struct StatusEpochJs {
    time_ms: i64,
    cn0_mean_all: f64,
    cn0_mean_used: f64,
    num_visible: u32,
    num_used: u32,
    constellations: Vec<ConstellationStatsJs>,
}

#[derive(Serialize, Tsify)]
struct ConstellationStatsJs {
    constellation: u8,
    constellation_name: String,
    cn0_mean: f64,
    visible: u32,
    used_in_fix: u32,
}

#[derive(Serialize, Tsify)]
struct FixEpochJs {
    time_ms: i64,
    accuracy_m: Option<f64>,
    vertical_accuracy_m: Option<f64>,
    speed_mps: Option<f64>,
}

#[derive(Serialize, Tsify)]
struct DrPointJs {
    time_ms: i64,
    latitude_deg: f64,
    longitude_deg: f64,
    altitude_m: f64,
    source: &'static str,
}

#[derive(Serialize, Tsify)]
struct SensorTimeSeries {
    accel: Vec<DecimatedSample<SensorXyz>>,
    gyro: Vec<DecimatedSample<SensorXyz>>,
    mag: Vec<DecimatedSample<SensorXyz>>,
    orientation: Vec<DecimatedSample<OrientationValue>>,
    rotation: Vec<DecimatedSample<RotationValue>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_basic_feed() {
        let mut proc = GnssLogProcessor::new();
        let chunk = b"# Header line\nFix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\n";
        let lines = proc.feed(chunk);
        assert_eq!(lines, 2);
        assert_eq!(proc.lines_parsed(), 2);
    }

    #[test]
    fn processor_split_chunks() {
        let mut proc = GnssLogProcessor::new();
        let chunk1 = b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,177164";
        let chunk2 = b"1748000,0.07,,2091905471128467,3.66,0,,,\n";

        let lines1 = proc.feed(chunk1);
        assert_eq!(lines1, 0);

        let lines2 = proc.feed(chunk2);
        assert_eq!(lines2, 1);
        assert_eq!(proc.lines_parsed(), 1);
    }

    #[test]
    fn processor_header_parsed() {
        let mut proc = GnssLogProcessor::new();
        let chunk = b"# \n# Header Description:\n# \n# Version: v3.1.1.2 Platform: 15 Manufacturer: SHARP Model: SH-M26 GNSS Hardware Model Name: qcom;MPSS.DE.3.1.1;\n# \nFix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\n";
        proc.feed(chunk);
        assert!(proc.header.is_some());
        assert_eq!(proc.header.as_ref().unwrap().model, "SH-M26");
    }

    #[test]
    fn processor_counts_records() {
        let mut proc = GnssLogProcessor::new();
        let chunk = b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\nFix,FLP,36.212,140.097,281.3,0.0,3.79,,1771641749000,0.07,,2091905471128467,3.66,0,,,\nNav,binary,data,here\n";
        proc.feed(chunk);
        assert_eq!(proc.fix_count, 2);
        assert_eq!(proc.fixes.len(), 2);
        assert_eq!(proc.fix_qualities.len(), 2);
        assert_eq!(proc.skipped_count, 1);
        // Both GPS and FLP are Primary
        assert_eq!(proc.fix_qualities[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities[1], FixQuality::Primary);
    }

    #[test]
    fn nlp_rejected_when_gps_recent() {
        let mut proc = GnssLogProcessor::new();
        // GPS fix at t=1000
        proc.feed(b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\n");
        // NLP fix 2s later (within 5s gap threshold) — should be Rejected
        proc.feed(b"Fix,NLP,36.500,140.500,,,400.0,,1771641750000,,,,,,,,\n");
        assert_eq!(proc.fixes.len(), 2);
        assert_eq!(proc.fix_qualities[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities[1], FixQuality::Rejected);
    }

    #[test]
    fn nlp_gap_fallback_when_no_recent_gps() {
        let mut proc = GnssLogProcessor::new();
        // GPS fix at t=1000
        proc.feed(b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\n");
        // NLP fix 10s later (beyond 5s gap threshold) — should be GapFallback
        proc.feed(b"Fix,NLP,36.500,140.500,,,82.5,,1771641758000,,,,,,,,\n");
        assert_eq!(proc.fixes.len(), 2);
        assert_eq!(proc.fix_qualities[0], FixQuality::Primary);
        assert_eq!(proc.fix_qualities[1], FixQuality::GapFallback);
    }

    #[test]
    fn nlp_gap_fallback_when_no_gps_at_all() {
        let mut proc = GnssLogProcessor::new();
        // NLP fix with no prior GPS/FLP — should be GapFallback
        proc.feed(b"Fix,NLP,36.500,140.500,,,82.5,,1771641758000,,,,,,,,\n");
        assert_eq!(proc.fixes.len(), 1);
        assert_eq!(proc.fix_qualities[0], FixQuality::GapFallback);
    }

    #[test]
    fn processor_status_time_annotation_and_aggregation() {
        let mut proc = GnssLogProcessor::new();
        let chunk = b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\nStatus,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1\n";
        proc.feed(chunk);
        assert_eq!(proc.status_count, 1);
        // Status is consumed by aggregator, not stored in a Vec
    }

    #[test]
    fn processor_empty_input() {
        let mut proc = GnssLogProcessor::new();
        let lines = proc.feed(b"");
        assert_eq!(lines, 0);
        assert_eq!(proc.lines_parsed(), 0);
    }

    #[test]
    fn processor_multiple_chunks_with_remainder() {
        let mut proc = GnssLogProcessor::new();

        let chunk1 = b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\nFix,GPS,36.213,140.098";
        proc.feed(chunk1);
        assert_eq!(proc.fixes.len(), 1);

        let chunk2 = b",282.0,1.0,4.0,,1771641749000,0.08,,2091905471128467,3.50,0,,,\n";
        proc.feed(chunk2);
        assert_eq!(proc.fixes.len(), 2);
    }

    #[test]
    fn processor_sensor_decimation() {
        let mut proc = GnssLogProcessor::new();

        // Feed 100 accel records at ~100Hz (10ms apart)
        for i in 0..100 {
            let t = 1771641748000i64 + i * 10;
            let line = format!(
                "UncalAccel,{t},2091905471128467,{},{},{},0.0,0.0,0.0,3",
                0.1 * i as f64,
                0.2 * i as f64,
                9.8 + 0.01 * i as f64,
            );
            proc.feed(format!("{line}\n").as_bytes());
        }

        assert_eq!(proc.uncal_accel_count, 100);
        // ~10 decimated samples (100Hz / 10 = 10Hz)
        let count = proc.accel_decimator.output_count();
        assert!(
            (9..=12).contains(&count),
            "expected ~10 decimated samples, got {count}"
        );
    }

    #[test]
    fn processor_dr_runs_inline() {
        let mut proc = GnssLogProcessor::new();

        // Feed a good Fix (establishes anchor)
        let fix1 = b"Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,\n";
        proc.feed(fix1);
        assert_eq!(proc.fix_count, 1);

        // Feed a degraded Fix (accuracy > 30m, triggers DR)
        let fix2 =
            b"Fix,GPS,36.212,140.097,281.3,0.0,50.0,,1771641750000,,,2091907471128467,30.0,0,,,\n";
        proc.feed(fix2);
        assert_eq!(proc.fix_count, 2);
    }

    #[test]
    fn processor_aggregator_runs_inline() {
        let mut proc = GnssLogProcessor::new();

        let chunk = b"\
Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Status,,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0
Fix,GPS,36.212,140.097,281.3,1.5,4.0,,1771641749000,0.82,25.9,2092092474651730,2.8,0,,,
Status,1771641749000,46,0,1,2,1575420030,30.00,192.285,31.194557,1,1,1,22.1
";

        proc.feed(chunk);
        assert_eq!(proc.fix_count, 2);
        assert_eq!(proc.status_count, 3);
    }

    // ─── SatelliteSnapshot tests ───

    #[test]
    fn satellite_snapshots_populated_from_status() {
        let mut proc = GnssLogProcessor::new();
        // Fix gives timestamp context for Status records with empty time
        let chunk = b"\
Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Status,,46,1,3,9,1600875010,28.40,10.0,45.0,1,1,1,21.0
";
        proc.feed(chunk);

        assert_eq!(proc.satellite_snapshots.len(), 2);

        // First satellite: GPS, svid=2
        let s0 = &proc.satellite_snapshots[0];
        assert_eq!(s0.constellation, trajix::types::ConstellationType::Gps);
        assert_eq!(s0.svid, 2);
        assert!((s0.azimuth_deg - 192.285).abs() < 0.001);
        assert!((s0.elevation_deg - 31.194557).abs() < 0.001);
        assert!((s0.cn0_dbhz - 25.70).abs() < 0.01);
        assert!(s0.used_in_fix);

        // Second satellite: GLONASS, svid=9
        let s1 = &proc.satellite_snapshots[1];
        assert_eq!(s1.constellation, trajix::types::ConstellationType::Glonass);
        assert_eq!(s1.svid, 9);
        assert!((s1.azimuth_deg - 10.0).abs() < 0.001);
        assert!((s1.elevation_deg - 45.0).abs() < 0.001);
        assert!((s1.cn0_dbhz - 28.40).abs() < 0.01);
        assert!(s1.used_in_fix);
    }

    #[test]
    fn satellite_snapshots_timestamp_inference() {
        let mut proc = GnssLogProcessor::new();
        // Status records have empty unix_time_ms; should be inferred from preceding Fix
        let chunk = b"\
Fix,GPS,36.212,140.097,281.3,0.0,3.79,,1771641748000,0.07,,2091905471128467,3.66,0,,,
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
Fix,GPS,36.212,140.097,281.3,1.5,4.0,,1771641749000,0.82,25.9,2092092474651730,2.8,0,,,
Status,,46,0,1,2,1575420030,30.00,180.0,50.0,1,1,1,22.1
";
        proc.feed(chunk);

        assert_eq!(proc.satellite_snapshots.len(), 2);
        // First Status: time inferred from Fix at 1771641748000
        assert_eq!(proc.satellite_snapshots[0].time_ms, 1771641748000);
        // Second Status: time inferred from Fix at 1771641749000
        assert_eq!(proc.satellite_snapshots[1].time_ms, 1771641749000);
    }

    #[test]
    fn satellite_snapshots_with_explicit_timestamp() {
        let mut proc = GnssLogProcessor::new();
        // Status with explicit timestamp
        let chunk = b"\
Status,1771641749000,46,0,1,2,1575420030,30.00,180.0,50.0,1,1,1,22.1
";
        proc.feed(chunk);

        assert_eq!(proc.satellite_snapshots.len(), 1);
        assert_eq!(proc.satellite_snapshots[0].time_ms, 1771641749000);
        assert!((proc.satellite_snapshots[0].azimuth_deg - 180.0).abs() < 0.001);
        assert!((proc.satellite_snapshots[0].elevation_deg - 50.0).abs() < 0.001);
    }

    #[test]
    fn satellite_snapshots_skipped_without_timestamp() {
        let mut proc = GnssLogProcessor::new();
        // Status with no timestamp and no preceding record to infer from
        let chunk = b"\
Status,,46,0,1,2,1575420030,25.70,192.285,31.194557,1,1,1,22.1
";
        proc.feed(chunk);

        // No timestamp available → snapshot not stored
        assert_eq!(proc.satellite_snapshots.len(), 0);
        // But status_count still incremented
        assert_eq!(proc.status_count, 1);
    }

    // ─── StreamingDecimator tests ───

    #[test]
    fn decimator_basic() {
        let mut d = StreamingDecimator::new(100);
        for i in 0..100 {
            d.push(1000 + i * 10, i as f64);
        }
        let result = d.finalize();

        assert!(
            result.len() >= 10 && result.len() <= 12,
            "expected ~10 samples, got {}",
            result.len()
        );
        assert_eq!(result[0].time_ms, 1000);
    }

    #[test]
    fn decimator_sparse_input() {
        let mut d = StreamingDecimator::new(100);
        d.push(1000, 1.0);
        d.push(2000, 2.0);
        d.push(3000, 3.0);
        let result = d.finalize();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn decimator_single_sample() {
        let mut d = StreamingDecimator::new(100);
        d.push(1000, 42.0);
        let result = d.finalize();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time_ms, 1000);
    }

    #[test]
    fn decimator_empty() {
        let d: StreamingDecimator<f64> = StreamingDecimator::new(100);
        let result = d.finalize();
        assert!(result.is_empty());
    }

    #[test]
    fn decimator_selects_nearest_to_center() {
        let mut d = StreamingDecimator::new(100);
        // First sample (always emitted)
        d.push(1000, 0.0);
        // Bin 1 = [1100, 1200), center = 1150
        d.push(1110, 1.0); // dist = 40
        d.push(1148, 2.0); // dist = 2 (closest!)
        d.push(1190, 3.0); // dist = 40
        // Bin 2 (forces flush of bin 1)
        d.push(1250, 4.0);

        let result = d.finalize();
        // First sample + bin 1 best + bin 2 best
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].time_ms, 1000);
        assert_eq!(result[1].time_ms, 1148); // closest to center
    }

    #[test]
    fn decimator_preserves_order() {
        let mut d = StreamingDecimator::new(100);
        for i in 0..50 {
            d.push(1000 + i * 20, i as f64);
        }
        let result = d.finalize();
        for window in result.windows(2) {
            assert!(
                window[0].time_ms < window[1].time_ms,
                "order violated: {} >= {}",
                window[0].time_ms,
                window[1].time_ms
            );
        }
    }
}
