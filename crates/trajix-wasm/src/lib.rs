use serde::Serialize;
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use trajix::dead_reckoning::PointSource;
use trajix::downsample::DecimatedSample;
use trajix::parser::header::HeaderInfo;
use trajix::pipeline::{GnssProcessor, OrientationValue, RotationValue, SensorXyz};
use trajix::quality::FixQuality;
use trajix::record::fix::FixRecord;
use trajix::record::status::SatelliteSnapshot;

// ────────────────────────────────────────────
// GnssLogProcessor: chunk-based WASM wrapper
// ────────────────────────────────────────────

/// Streaming GNSS log processor for browser use.
///
/// Thin wrapper around [`trajix::pipeline::GnssProcessor`] that adds
/// chunk-level byte buffering for the browser `feed(Uint8Array)` pattern.
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
    remainder: Vec<u8>,
    bytes_fed: u64,
    processor: GnssProcessor,
}

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
            bytes_fed: 0,
            processor: GnssProcessor::new(),
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
                self.processor.process_line(line);
                lines_this_chunk += 1;
                start = i + 1;
            }
        }

        // Save remainder (incomplete line at end of chunk)
        if start < data.len() {
            self.remainder = data[start..].to_vec();
        }

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
                self.processor.process_line(line);
            }
        }

        // Finalize the core processor
        let result = self.processor.finalize();

        // Map to JS-specific types
        let js_result = ProcessingResult {
            header: result.header,
            lines_parsed: result.lines_parsed,
            record_counts: RecordCounts {
                fix: result.record_counts.fix,
                status: result.record_counts.status,
                raw: result.record_counts.raw,
                uncal_accel: result.record_counts.uncal_accel,
                uncal_gyro: result.record_counts.uncal_gyro,
                uncal_mag: result.record_counts.uncal_mag,
                orientation: result.record_counts.orientation,
                game_rotation: result.record_counts.game_rotation,
                skipped: result.record_counts.skipped,
                errors: result.record_counts.errors,
            },
            fixes: result.fixes,
            fix_qualities: result.fix_qualities,
            status_epochs: result
                .status_epochs
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
            fix_epochs: result
                .fix_epochs
                .into_iter()
                .map(|e| FixEpochJs {
                    time_ms: e.time_ms,
                    accuracy_m: e.accuracy_m,
                    vertical_accuracy_m: e.vertical_accuracy_m,
                    speed_mps: e.speed_mps,
                })
                .collect(),
            dr_trajectory: result
                .dr_trajectory
                .into_iter()
                .map(|p| TrajectoryPointJs {
                    time_ms: p.time_ms,
                    latitude_deg: p.latitude_deg,
                    longitude_deg: p.longitude_deg,
                    altitude_m: p.altitude_m,
                    source: match p.source {
                        PointSource::Gnss => "gnss",
                        PointSource::DeadReckoning => "dr",
                    },
                })
                .collect(),
            satellite_snapshots: result.satellite_snapshots,
            sensor_time_series: SensorTimeSeries {
                accel: result.sensor_time_series.accel,
                gyro: result.sensor_time_series.gyro,
                mag: result.sensor_time_series.mag,
                orientation: result.sensor_time_series.orientation,
                rotation: result.sensor_time_series.rotation,
            },
        };

        serde_wasm_bindgen::to_value(&js_result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Number of lines parsed so far.
    pub fn lines_parsed(&self) -> u64 {
        self.processor.lines_parsed()
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

// ────────────────────────────────────────────
// JS-specific result types (Tsify + serde-wasm-bindgen)
// ────────────────────────────────────────────

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
struct ProcessingResult {
    header: Option<HeaderInfo>,
    lines_parsed: u64,
    record_counts: RecordCounts,
    fixes: Vec<FixRecord>,
    fix_qualities: Vec<FixQuality>,
    status_epochs: Vec<StatusEpochJs>,
    fix_epochs: Vec<FixEpochJs>,
    dr_trajectory: Vec<TrajectoryPointJs>,
    satellite_snapshots: Vec<SatelliteSnapshot>,
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
struct TrajectoryPointJs {
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

// ────────────────────────────────────────────
// Tests (chunk-level buffering only)
// ────────────────────────────────────────────

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
        let header = proc.processor.header();
        assert!(header.is_some());
        assert_eq!(header.unwrap().model, "SH-M26");
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
        assert_eq!(proc.processor.fixes().len(), 1);

        let chunk2 = b",282.0,1.0,4.0,,1771641749000,0.08,,2091905471128467,3.50,0,,,\n";
        proc.feed(chunk2);
        assert_eq!(proc.processor.fixes().len(), 2);
    }
}
