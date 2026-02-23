//! Altitude processing: spike removal, smoothing, and vertical velocity analysis.
//!
//! GPS altitude has inherent vertical noise (2-3x worse than horizontal).
//! Additionally, GPS and FLP providers interleave at sub-second intervals
//! with systematically different altitudes (~1.6m median, up to ~80m worst case).
//!
//! Two-pass pipeline:
//! 1. Spike filter (running median): removes extreme outliers (>30m)
//! 2. Smoothing (time-aware moving average): reduces residual jitter

use crate::record::fix::FixRecord;
use crate::stats::{PercentileStats, percentiles};
use crate::types::FixProvider;

// ────────────────────────────────────────────
// Spike filter
// ────────────────────────────────────────────

/// Configuration for the altitude spike filter.
#[derive(Debug, Clone)]
pub struct SpikeFilterConfig {
    /// Maximum allowed deviation from local median (meters). Default: 30.0
    pub deviation_threshold_m: f64,
    /// Half-window size for median computation. Window = 2*half_window+1. Default: 5
    pub half_window: usize,
}

impl Default for SpikeFilterConfig {
    fn default() -> Self {
        Self {
            deviation_threshold_m: 30.0,
            half_window: 5,
        }
    }
}

/// Statistics from altitude spike filtering.
#[derive(Debug, Clone)]
pub struct SpikeFilterStats {
    /// Number of points replaced with median values.
    pub points_replaced: usize,
    /// Maximum deviation observed across all points (meters).
    pub max_deviation: f64,
}

/// Filter altitude spikes using a running median approach.
///
/// For each point, computes the median of surrounding ±half_window points.
/// If the point's height deviates from the median by more than
/// `config.deviation_threshold_m`, it is replaced with the median.
///
/// Takes only `heights` (no timestamps needed — purely index-based).
/// Works with raw FixRecord altitudes or geoid-corrected heights.
pub fn filter_altitude_spikes(
    heights: &[f64],
    config: &SpikeFilterConfig,
) -> (Vec<f64>, SpikeFilterStats) {
    if heights.len() <= 2 {
        return (
            heights.to_vec(),
            SpikeFilterStats {
                points_replaced: 0,
                max_deviation: 0.0,
            },
        );
    }

    let n = heights.len();
    let mut filtered = vec![0.0; n];
    let mut points_replaced = 0;
    let mut max_deviation = 0.0_f64;

    for i in 0..n {
        let lo = i.saturating_sub(config.half_window);
        let hi = (i + config.half_window).min(n - 1);

        let mut window: Vec<f64> = (lo..=hi).map(|j| heights[j]).collect();
        window.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mid = window.len() / 2;
        let median = if window.len() % 2 == 1 {
            window[mid]
        } else {
            (window[mid - 1] + window[mid]) / 2.0
        };

        let deviation = (heights[i] - median).abs();
        if deviation > config.deviation_threshold_m {
            filtered[i] = median;
            points_replaced += 1;
            max_deviation = max_deviation.max(deviation);
        } else {
            filtered[i] = heights[i];
        }
    }

    (
        filtered,
        SpikeFilterStats {
            points_replaced,
            max_deviation,
        },
    )
}

// ────────────────────────────────────────────
// Altitude smoothing (time-aware moving average)
// ────────────────────────────────────────────

/// Configuration for altitude smoothing.
#[derive(Debug, Clone)]
pub struct SmoothConfig {
    /// Half-window size (points). Window = 2*half_window+1. Default: 5
    pub half_window: usize,
    /// Time gap threshold (ms). Don't average across gaps larger than this. Default: 3000
    pub gap_threshold_ms: i64,
}

impl Default for SmoothConfig {
    fn default() -> Self {
        Self {
            half_window: 5,
            gap_threshold_ms: 3000,
        }
    }
}

/// Smooth altitudes using a time-aware moving average.
///
/// For each point, averages the surrounding ±half_window points, but only
/// includes neighbors within `gap_threshold_ms` of the current point
/// (to avoid averaging across time gaps).
///
/// Takes parallel slices: `timestamps_ms[i]` corresponds to `heights[i]`.
///
/// # Panics
///
/// Panics if `timestamps_ms.len() != heights.len()`.
pub fn smooth_altitudes(timestamps_ms: &[i64], heights: &[f64], config: &SmoothConfig) -> Vec<f64> {
    assert_eq!(timestamps_ms.len(), heights.len());

    let n = heights.len();
    if n <= 2 {
        return heights.to_vec();
    }

    let mut smoothed = vec![0.0; n];

    for i in 0..n {
        let t_center = timestamps_ms[i];
        let lo = i.saturating_sub(config.half_window);
        let hi = (i + config.half_window).min(n - 1);

        let mut sum = 0.0;
        let mut count = 0;

        for j in lo..=hi {
            if (timestamps_ms[j] - t_center).abs() <= config.gap_threshold_ms {
                sum += heights[j];
                count += 1;
            }
        }

        smoothed[i] = if count > 0 {
            sum / count as f64
        } else {
            heights[i]
        };
    }

    smoothed
}

// ────────────────────────────────────────────
// Vertical velocity analysis
// ────────────────────────────────────────────

/// A single vertical velocity measurement between consecutive altitude points.
#[derive(Debug, Clone)]
pub struct VerticalVelocity {
    /// Index of the second point in the source array.
    pub index: usize,
    /// Vertical velocity in m/s (signed: positive = ascending).
    pub velocity_mps: f64,
    /// Time delta between the two points in seconds.
    pub dt_s: f64,
    /// Altitude change in meters.
    pub delta_alt_m: f64,
    /// Altitude of the "before" point.
    pub alt_before_m: f64,
    /// Altitude of the "after" point.
    pub alt_after_m: f64,
}

/// Configuration for vertical velocity analysis.
#[derive(Debug, Clone)]
pub struct VerticalVelocityConfig {
    /// Maximum time gap between consecutive points to analyze (seconds).
    /// Pairs with dt > max_gap_s are skipped. Default: 60.0
    pub max_gap_s: f64,
    /// Thresholds (m/s) for spike counting. Default: [5.0, 10.0, 20.0, 50.0, 100.0]
    pub spike_thresholds_mps: Vec<f64>,
    /// Threshold for detecting spike segments. Default: 10.0 m/s
    pub segment_threshold_mps: f64,
    /// Index tolerance for grouping spike indices into segments. Default: 2
    pub segment_index_tolerance: usize,
}

impl Default for VerticalVelocityConfig {
    fn default() -> Self {
        Self {
            max_gap_s: 60.0,
            spike_thresholds_mps: vec![5.0, 10.0, 20.0, 50.0, 100.0],
            segment_threshold_mps: 10.0,
            segment_index_tolerance: 2,
        }
    }
}

/// Result of vertical velocity analysis.
#[derive(Debug, Clone)]
pub struct VerticalVelocityReport {
    /// All computed vertical velocities.
    pub velocities: Vec<VerticalVelocity>,
    /// Percentile statistics of |vertical velocity| (m/s).
    pub abs_velocity_stats: Option<PercentileStats>,
    /// Number of velocity measurements exceeding each threshold.
    /// Each entry is `(threshold_mps, count)`.
    pub spike_counts: Vec<(f64, usize)>,
    /// Contiguous segments of spike indices `(start_idx, end_idx)` inclusive.
    pub spike_segments: Vec<(usize, usize)>,
}

/// Analyze vertical velocity between consecutive altitude samples.
///
/// Takes parallel slices: `timestamps_ms[i]` and `altitudes_m[i]` correspond.
/// Skips pairs where dt <= 0 or dt > `config.max_gap_s`.
///
/// # Panics
///
/// Panics if `timestamps_ms.len() != altitudes_m.len()`.
pub fn analyze_vertical_velocity(
    timestamps_ms: &[i64],
    altitudes_m: &[f64],
    config: &VerticalVelocityConfig,
) -> VerticalVelocityReport {
    assert_eq!(timestamps_ms.len(), altitudes_m.len());

    let n = timestamps_ms.len();
    let mut velocities = Vec::new();

    for i in 1..n {
        let dt_s = (timestamps_ms[i] - timestamps_ms[i - 1]) as f64 / 1000.0;
        if dt_s <= 0.0 || dt_s > config.max_gap_s {
            continue;
        }
        let d_alt = altitudes_m[i] - altitudes_m[i - 1];
        velocities.push(VerticalVelocity {
            index: i,
            velocity_mps: d_alt / dt_s,
            dt_s,
            delta_alt_m: d_alt,
            alt_before_m: altitudes_m[i - 1],
            alt_after_m: altitudes_m[i],
        });
    }

    // Percentile stats on |velocity|
    let mut abs_vv: Vec<f64> = velocities.iter().map(|v| v.velocity_mps.abs()).collect();
    abs_vv.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let abs_velocity_stats = if abs_vv.is_empty() {
        None
    } else {
        Some(percentiles(&abs_vv))
    };

    // Spike counts by threshold
    let spike_counts: Vec<(f64, usize)> = config
        .spike_thresholds_mps
        .iter()
        .map(|&t| (t, abs_vv.iter().filter(|&&v| v > t).count()))
        .collect();

    // Spike segment detection
    let mut spike_indices: Vec<usize> = Vec::new();
    for v in &velocities {
        if v.velocity_mps.abs() > config.segment_threshold_mps {
            if v.index > 0 {
                spike_indices.push(v.index - 1);
            }
            spike_indices.push(v.index);
        }
    }
    spike_indices.sort();
    spike_indices.dedup();

    let spike_segments = group_segments(&spike_indices, config.segment_index_tolerance);

    VerticalVelocityReport {
        velocities,
        abs_velocity_stats,
        spike_counts,
        spike_segments,
    }
}

/// Group sorted, deduplicated indices into contiguous segments.
/// Indices within `tolerance` of each other are merged into one segment.
fn group_segments(indices: &[usize], tolerance: usize) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    let mut seg_start: Option<usize> = None;
    let mut seg_end = 0usize;

    for &idx in indices {
        match seg_start {
            None => {
                seg_start = Some(idx);
                seg_end = idx;
            }
            Some(_) => {
                if idx <= seg_end + tolerance {
                    seg_end = idx;
                } else {
                    segments.push((seg_start.unwrap(), seg_end));
                    seg_start = Some(idx);
                    seg_end = idx;
                }
            }
        }
    }
    if let Some(s) = seg_start {
        segments.push((s, seg_end));
    }

    segments
}

// ────────────────────────────────────────────
// Provider interleaving analysis
// ────────────────────────────────────────────

/// GPS/FLP provider interleaving statistics.
#[derive(Debug, Clone)]
pub struct ProviderInterleavingReport {
    /// Number of provider switch transitions.
    pub transition_count: usize,
    /// Percentile stats of |altitude difference| at provider switch (meters).
    pub abs_delta_alt_stats: Option<PercentileStats>,
    /// GPS altitude percentile stats.
    pub gps_altitude_stats: Option<PercentileStats>,
    /// FLP altitude percentile stats.
    pub flp_altitude_stats: Option<PercentileStats>,
}

/// Analyze GPS/FLP provider interleaving altitude differences.
///
/// Fixes should be sorted by `unix_time_ms` and pre-filtered to GPS+FLP with altitude.
pub fn analyze_provider_interleaving(fixes: &[FixRecord]) -> ProviderInterleavingReport {
    let mut gps_alts: Vec<f64> = Vec::new();
    let mut flp_alts: Vec<f64> = Vec::new();
    let mut abs_diffs: Vec<f64> = Vec::new();

    for i in 1..fixes.len() {
        let prev = &fixes[i - 1];
        let curr = &fixes[i];

        if prev.provider != curr.provider
            && let (Some(alt_prev), Some(alt_curr)) = (prev.altitude_m, curr.altitude_m)
        {
            abs_diffs.push((alt_curr - alt_prev).abs());
        }

        match curr.provider {
            FixProvider::Gps => {
                if let Some(alt) = curr.altitude_m {
                    gps_alts.push(alt);
                }
            }
            FixProvider::Flp => {
                if let Some(alt) = curr.altitude_m {
                    flp_alts.push(alt);
                }
            }
            _ => {}
        }
    }

    abs_diffs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    gps_alts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    flp_alts.sort_by(|a, b| a.partial_cmp(b).unwrap());

    ProviderInterleavingReport {
        transition_count: abs_diffs.len(),
        abs_delta_alt_stats: if abs_diffs.is_empty() {
            None
        } else {
            Some(percentiles(&abs_diffs))
        },
        gps_altitude_stats: if gps_alts.is_empty() {
            None
        } else {
            Some(percentiles(&gps_alts))
        },
        flp_altitude_stats: if flp_alts.is_empty() {
            None
        } else {
            Some(percentiles(&flp_alts))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Spike filter tests ───

    #[test]
    fn spike_filter_empty() {
        let (filtered, stats) = filter_altitude_spikes(&[], &SpikeFilterConfig::default());
        assert!(filtered.is_empty());
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_single() {
        let (filtered, stats) = filter_altitude_spikes(&[100.0], &SpikeFilterConfig::default());
        assert_eq!(filtered, vec![100.0]);
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_two_points() {
        let (filtered, stats) =
            filter_altitude_spikes(&[100.0, 200.0], &SpikeFilterConfig::default());
        assert_eq!(filtered, vec![100.0, 200.0]);
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_no_spikes() {
        let heights: Vec<f64> = (0..10).map(|i| 100.0 + i as f64).collect();
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(filtered, heights);
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_one_spike_at_center() {
        // 10 points at 100m with a 200m spike at index 5
        let mut heights = vec![100.0; 10];
        heights[5] = 200.0;
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 1);
        assert!((stats.max_deviation - 100.0).abs() < 0.1);
        assert!((filtered[5] - 100.0).abs() < 0.1);
        // Other points unchanged
        for (i, &h) in filtered.iter().enumerate() {
            if i != 5 {
                assert_eq!(h, 100.0);
            }
        }
    }

    #[test]
    fn spike_filter_spike_at_start() {
        let mut heights = vec![100.0; 10];
        heights[0] = 200.0;
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 1);
        assert!((filtered[0] - 100.0).abs() < 0.1);
    }

    #[test]
    fn spike_filter_spike_at_end() {
        let mut heights = vec![100.0; 10];
        heights[9] = 200.0;
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 1);
        assert!((filtered[9] - 100.0).abs() < 0.1);
    }

    #[test]
    fn spike_filter_gradual_climb_preserved() {
        // Linear ascent: should not be filtered
        let heights: Vec<f64> = (0..20).map(|i| 100.0 + i as f64 * 5.0).collect();
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 0);
        assert_eq!(filtered, heights);
    }

    #[test]
    fn spike_filter_threshold_boundary() {
        // Deviation exactly at threshold: should NOT be replaced
        let mut heights = vec![100.0; 11];
        heights[5] = 130.0; // exactly 30m deviation (= threshold)
        let (_, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_just_over_threshold() {
        let mut heights = vec![100.0; 11];
        heights[5] = 130.1; // slightly over threshold
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 1);
        assert!((filtered[5] - 100.0).abs() < 0.1);
    }

    #[test]
    fn spike_filter_custom_config() {
        let mut heights = vec![100.0; 11];
        heights[5] = 115.0; // 15m deviation
        let config = SpikeFilterConfig {
            deviation_threshold_m: 10.0,
            half_window: 3,
        };
        let (filtered, stats) = filter_altitude_spikes(&heights, &config);
        assert_eq!(stats.points_replaced, 1);
        assert!((filtered[5] - 100.0).abs() < 0.1);
    }

    #[test]
    fn spike_filter_gps_flp_jitter_preserved() {
        // GPS/FLP interleave with ~2-3m jitter — should NOT be filtered
        let heights: Vec<f64> = (0..20)
            .map(|i| 100.0 + if i % 2 == 0 { 1.5 } else { -1.5 })
            .collect();
        let (_, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 0);
    }

    #[test]
    fn spike_filter_mountain_with_spike() {
        // Simulated mountain climb with a 150m spike
        let mut heights: Vec<f64> = (0..50).map(|i| 500.0 + i as f64 * 10.0).collect();
        heights[25] += 150.0; // spike at midpoint
        let expected_at_25 = 500.0 + 25.0 * 10.0; // 750.0
        let (filtered, stats) = filter_altitude_spikes(&heights, &SpikeFilterConfig::default());
        assert_eq!(stats.points_replaced, 1);
        // Replaced value should be close to the trend
        assert!((filtered[25] - expected_at_25).abs() < 60.0);
    }

    // ─── Smoothing tests ───

    #[test]
    fn smooth_empty() {
        let result = smooth_altitudes(&[], &[], &SmoothConfig::default());
        assert!(result.is_empty());
    }

    #[test]
    fn smooth_single() {
        let result = smooth_altitudes(&[0], &[100.0], &SmoothConfig::default());
        assert_eq!(result, vec![100.0]);
    }

    #[test]
    fn smooth_two_points() {
        let result = smooth_altitudes(&[0, 1000], &[100.0, 200.0], &SmoothConfig::default());
        assert_eq!(result, vec![100.0, 200.0]);
    }

    #[test]
    fn smooth_constant_unchanged() {
        let ts: Vec<i64> = (0..10).map(|i| i * 1000).collect();
        let heights = vec![100.0; 10];
        let result = smooth_altitudes(&ts, &heights, &SmoothConfig::default());
        for h in &result {
            assert!((h - 100.0).abs() < 1e-10);
        }
    }

    #[test]
    fn smooth_linear_preserved() {
        // Moving average of a linear function = the same linear function (interior points)
        let ts: Vec<i64> = (0..20).map(|i| i * 1000).collect();
        let heights: Vec<f64> = (0..20).map(|i| 100.0 + i as f64 * 2.0).collect();
        let result = smooth_altitudes(&ts, &heights, &SmoothConfig::default());
        // Interior points (5..15) should be very close to original
        for i in 5..15 {
            assert!(
                (result[i] - heights[i]).abs() < 0.1,
                "at {i}: expected {:.1}, got {:.1}",
                heights[i],
                result[i]
            );
        }
    }

    #[test]
    fn smooth_respects_time_gap() {
        // Two groups separated by a large time gap
        let ts = vec![
            0, 1000, 2000, 3000, 4000, 100_000, 101_000, 102_000, 103_000, 104_000,
        ];
        let heights = vec![
            100.0, 100.0, 100.0, 100.0, 100.0, 200.0, 200.0, 200.0, 200.0, 200.0,
        ];
        let result = smooth_altitudes(&ts, &heights, &SmoothConfig::default());
        // Points in group 1 should not be influenced by group 2
        for (i, &val) in result.iter().enumerate().take(5) {
            assert!((val - 100.0).abs() < 0.1, "group1[{i}] = {val:.1}",);
        }
        for (i, &val) in result.iter().enumerate().take(10).skip(5) {
            assert!((val - 200.0).abs() < 0.1, "group2[{i}] = {val:.1}",);
        }
    }

    #[test]
    fn smooth_jitter_reduction() {
        // Alternating ±3m jitter at 1Hz — smoothing should reduce amplitude
        let ts: Vec<i64> = (0..20).map(|i| i * 1000).collect();
        let heights: Vec<f64> = (0..20)
            .map(|i| 100.0 + if i % 2 == 0 { 3.0 } else { -3.0 })
            .collect();
        let result = smooth_altitudes(&ts, &heights, &SmoothConfig::default());
        // After smoothing, amplitude should be much less than 3m for interior points
        for (i, &val) in result.iter().enumerate().take(15).skip(5) {
            assert!(
                (val - 100.0).abs() < 2.0,
                "at {i}: expected ~100, got {val:.1}",
            );
        }
    }

    #[test]
    fn smooth_custom_gap_threshold() {
        // Gap threshold of 500ms — neighbors at 1000ms apart should be excluded
        let ts: Vec<i64> = (0..5).map(|i| i * 1000).collect();
        let heights = vec![100.0, 110.0, 120.0, 130.0, 140.0];
        let config = SmoothConfig {
            half_window: 2,
            gap_threshold_ms: 500,
        };
        let result = smooth_altitudes(&ts, &heights, &config);
        // Each point can only see itself (neighbors >500ms away)
        for i in 0..5 {
            assert!(
                (result[i] - heights[i]).abs() < 1e-10,
                "at {i}: expected {:.1}, got {:.1}",
                heights[i],
                result[i]
            );
        }
    }

    // ─── Vertical velocity analysis tests ───

    #[test]
    fn vv_empty() {
        let report = analyze_vertical_velocity(&[], &[], &VerticalVelocityConfig::default());
        assert!(report.velocities.is_empty());
        assert!(report.abs_velocity_stats.is_none());
    }

    #[test]
    fn vv_single_point() {
        let report = analyze_vertical_velocity(&[0], &[100.0], &VerticalVelocityConfig::default());
        assert!(report.velocities.is_empty());
    }

    #[test]
    fn vv_known_velocity() {
        // 10m rise over 2 seconds = 5 m/s
        let report = analyze_vertical_velocity(
            &[0, 2000],
            &[100.0, 110.0],
            &VerticalVelocityConfig::default(),
        );
        assert_eq!(report.velocities.len(), 1);
        assert!((report.velocities[0].velocity_mps - 5.0).abs() < 1e-10);
        assert!((report.velocities[0].dt_s - 2.0).abs() < 1e-10);
        assert!((report.velocities[0].delta_alt_m - 10.0).abs() < 1e-10);
    }

    #[test]
    fn vv_descending() {
        // Descending: negative velocity
        let report = analyze_vertical_velocity(
            &[0, 1000],
            &[200.0, 190.0],
            &VerticalVelocityConfig::default(),
        );
        assert_eq!(report.velocities.len(), 1);
        assert!((report.velocities[0].velocity_mps - (-10.0)).abs() < 1e-10);
    }

    #[test]
    fn vv_skip_large_gap() {
        // Gap > 60s should be skipped
        let report = analyze_vertical_velocity(
            &[0, 61_000],
            &[100.0, 200.0],
            &VerticalVelocityConfig::default(),
        );
        assert!(report.velocities.is_empty());
    }

    #[test]
    fn vv_skip_zero_dt() {
        let report = analyze_vertical_velocity(
            &[1000, 1000],
            &[100.0, 200.0],
            &VerticalVelocityConfig::default(),
        );
        assert!(report.velocities.is_empty());
    }

    #[test]
    fn vv_spike_counts() {
        // 3 pairs: 2 m/s, 15 m/s, 60 m/s
        let report = analyze_vertical_velocity(
            &[0, 1000, 2000, 3000],
            &[100.0, 102.0, 117.0, 177.0],
            &VerticalVelocityConfig::default(),
        );
        assert_eq!(report.velocities.len(), 3);
        // Check spike counts: >5 should be 2, >10 should be 2, >20 should be 1, >50 should be 1
        let find_count = |t: f64| -> usize {
            report
                .spike_counts
                .iter()
                .find(|&&(th, _)| (th - t).abs() < 0.01)
                .unwrap()
                .1
        };
        assert_eq!(find_count(5.0), 2);
        assert_eq!(find_count(10.0), 2);
        assert_eq!(find_count(20.0), 1);
        assert_eq!(find_count(50.0), 1);
        assert_eq!(find_count(100.0), 0);
    }

    #[test]
    fn vv_spike_segments() {
        // Spike at indices 3,4,5 then gap, then spike at 10,11
        let n = 15;
        let mut alts = vec![100.0; n];
        let ts: Vec<i64> = (0..n).map(|i| i as i64 * 1000).collect();
        // Create spikes >10 m/s at transitions 2→3, 3→4, 4→5, and 9→10, 10→11
        alts[3] = 120.0; // +20m in 1s = 20 m/s
        alts[4] = 140.0;
        alts[5] = 160.0;
        alts[6] = 100.0; // drops back
        alts[10] = 120.0;
        alts[11] = 140.0;
        alts[12] = 100.0;

        let report = analyze_vertical_velocity(&ts, &alts, &VerticalVelocityConfig::default());
        // Should have 2 spike segments
        assert!(
            report.spike_segments.len() >= 2,
            "expected >=2 segments, got {:?}",
            report.spike_segments
        );
    }

    // ─── Provider interleaving tests ───

    fn make_fix(provider: FixProvider, time_ms: i64, altitude: Option<f64>) -> FixRecord {
        FixRecord {
            provider,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
            altitude_m: altitude,
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
    fn interleaving_empty() {
        let report = analyze_provider_interleaving(&[]);
        assert_eq!(report.transition_count, 0);
        assert!(report.abs_delta_alt_stats.is_none());
    }

    #[test]
    fn interleaving_single_provider() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, Some(100.0)),
            make_fix(FixProvider::Gps, 1000, Some(101.0)),
            make_fix(FixProvider::Gps, 2000, Some(102.0)),
        ];
        let report = analyze_provider_interleaving(&fixes);
        assert_eq!(report.transition_count, 0);
    }

    #[test]
    fn interleaving_alternating() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, Some(100.0)),
            make_fix(FixProvider::Flp, 500, Some(102.0)),
            make_fix(FixProvider::Gps, 1000, Some(101.0)),
            make_fix(FixProvider::Flp, 1500, Some(103.0)),
        ];
        let report = analyze_provider_interleaving(&fixes);
        assert_eq!(report.transition_count, 3);
        assert!(report.abs_delta_alt_stats.is_some());
        assert!(report.gps_altitude_stats.is_some());
        assert!(report.flp_altitude_stats.is_some());
        // GPS altitudes: 101.0 (index 2 onwards)
        // FLP altitudes: 102.0, 103.0
        assert_eq!(report.flp_altitude_stats.as_ref().unwrap().min, 102.0);
    }

    #[test]
    fn interleaving_no_altitude() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Flp, 500, None),
        ];
        let report = analyze_provider_interleaving(&fixes);
        assert_eq!(report.transition_count, 0); // no altitude data → no diffs
    }
}
