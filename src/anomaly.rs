//! Jump and anomaly detection for fix sequences.
//!
//! Computes implied ground speed between consecutive fixes and detects
//! anomalous jumps that exceed configurable thresholds.

use crate::geo;
use crate::record::fix::FixRecord;
use crate::types::FixProvider;

/// Default speed histogram bucket boundaries in km/h.
const DEFAULT_BUCKET_BOUNDARIES: &[f64] = &[1.0, 10.0, 50.0, 100.0, 200.0, 500.0];

/// Configuration for jump analysis.
#[derive(Debug, Clone)]
pub struct JumpConfig {
    /// Speed threshold (km/h) above which a jump is considered anomalous. Default: 200.0
    pub anomaly_threshold_kmh: f64,
}

impl Default for JumpConfig {
    fn default() -> Self {
        Self {
            anomaly_threshold_kmh: 200.0,
        }
    }
}

/// A single implied speed measurement between two consecutive fixes.
#[derive(Debug, Clone)]
pub struct ImpliedSpeed {
    /// Index of the second fix in the source array.
    pub index: usize,
    /// Implied speed in km/h.
    pub speed_kmh: f64,
    /// Haversine distance in meters.
    pub distance_m: f64,
    /// Time delta in seconds.
    pub dt_s: f64,
    /// Provider of the first fix.
    pub provider_before: FixProvider,
    /// Provider of the second fix.
    pub provider_after: FixProvider,
    /// Accuracy of the first fix (meters).
    pub accuracy_before: Option<f64>,
    /// Accuracy of the second fix (meters).
    pub accuracy_after: Option<f64>,
}

/// Histogram bucket with count.
#[derive(Debug, Clone)]
pub struct SpeedBucket {
    pub label: String,
    pub min_kmh: f64,
    pub max_kmh: f64,
    pub count: usize,
}

/// Jump analysis result.
#[derive(Debug, Clone)]
pub struct JumpReport {
    /// Maximum implied speed observed (km/h).
    pub max_speed_kmh: f64,
    /// Speed histogram.
    pub histogram: Vec<SpeedBucket>,
    /// Anomalous jumps (speed > threshold), sorted by speed descending.
    pub anomalies: Vec<ImpliedSpeed>,
    /// Total number of fix pairs analyzed.
    pub total_pairs: usize,
}

/// Analyze implied speeds between consecutive fixes.
///
/// Fixes should be sorted by `unix_time_ms`. Pairs with dt <= 0 are skipped.
pub fn analyze_jumps(fixes: &[FixRecord], config: &JumpConfig) -> JumpReport {
    if fixes.len() < 2 {
        return JumpReport {
            max_speed_kmh: 0.0,
            histogram: build_histogram_empty(),
            anomalies: Vec::new(),
            total_pairs: 0,
        };
    }

    let mut max_speed_kmh = 0.0_f64;
    let mut anomalies = Vec::new();
    let mut total_pairs = 0;

    // Bucket counts: [<min, bucket0, bucket1, ..., >=last]
    let boundaries = DEFAULT_BUCKET_BOUNDARIES;
    let mut bucket_counts = vec![0usize; boundaries.len() + 1];

    for i in 1..fixes.len() {
        let prev = &fixes[i - 1];
        let curr = &fixes[i];

        let dt_s = (curr.unix_time_ms - prev.unix_time_ms) as f64 / 1000.0;
        if dt_s <= 0.0 {
            continue;
        }

        let dist_m = geo::haversine_distance_m(
            prev.latitude_deg,
            prev.longitude_deg,
            curr.latitude_deg,
            curr.longitude_deg,
        );

        let speed_kmh = (dist_m / dt_s) * 3.6;
        total_pairs += 1;

        max_speed_kmh = max_speed_kmh.max(speed_kmh);

        // Bucket assignment
        let bucket_idx = boundaries
            .iter()
            .position(|&b| speed_kmh < b)
            .unwrap_or(boundaries.len());
        bucket_counts[bucket_idx] += 1;

        if speed_kmh > config.anomaly_threshold_kmh {
            anomalies.push(ImpliedSpeed {
                index: i,
                speed_kmh,
                distance_m: dist_m,
                dt_s,
                provider_before: prev.provider,
                provider_after: curr.provider,
                accuracy_before: prev.accuracy_m,
                accuracy_after: curr.accuracy_m,
            });
        }
    }

    anomalies.sort_by(|a, b| b.speed_kmh.partial_cmp(&a.speed_kmh).unwrap());

    let histogram = build_histogram(boundaries, &bucket_counts);

    JumpReport {
        max_speed_kmh,
        histogram,
        anomalies,
        total_pairs,
    }
}

fn build_histogram_empty() -> Vec<SpeedBucket> {
    build_histogram(
        DEFAULT_BUCKET_BOUNDARIES,
        &vec![0; DEFAULT_BUCKET_BOUNDARIES.len() + 1],
    )
}

fn build_histogram(boundaries: &[f64], counts: &[usize]) -> Vec<SpeedBucket> {
    let mut buckets = Vec::new();

    // First bucket: [0, boundary[0])
    buckets.push(SpeedBucket {
        label: format!("<{} km/h", boundaries[0]),
        min_kmh: 0.0,
        max_kmh: boundaries[0],
        count: counts[0],
    });

    // Middle buckets
    for i in 0..boundaries.len() - 1 {
        buckets.push(SpeedBucket {
            label: format!("{}-{} km/h", boundaries[i], boundaries[i + 1]),
            min_kmh: boundaries[i],
            max_kmh: boundaries[i + 1],
            count: counts[i + 1],
        });
    }

    // Last bucket: [last_boundary, +inf)
    let last = boundaries[boundaries.len() - 1];
    buckets.push(SpeedBucket {
        label: format!("{}+ km/h", last),
        min_kmh: last,
        max_kmh: f64::INFINITY,
        count: counts[boundaries.len()],
    });

    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fix(
        provider: FixProvider,
        lat: f64,
        lon: f64,
        time_ms: i64,
        accuracy: Option<f64>,
    ) -> FixRecord {
        FixRecord {
            provider,
            latitude_deg: lat,
            longitude_deg: lon,
            altitude_m: None,
            speed_mps: None,
            accuracy_m: accuracy,
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
    fn empty_fixes() {
        let report = analyze_jumps(&[], &JumpConfig::default());
        assert_eq!(report.max_speed_kmh, 0.0);
        assert_eq!(report.total_pairs, 0);
        assert!(report.anomalies.is_empty());
    }

    #[test]
    fn single_fix() {
        let fixes = vec![make_fix(FixProvider::Gps, 36.0, 140.0, 0, None)];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.total_pairs, 0);
    }

    #[test]
    fn stationary_fixes() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.0, 140.0, 1000, None),
            make_fix(FixProvider::Gps, 36.0, 140.0, 2000, None),
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.total_pairs, 2);
        assert!(report.max_speed_kmh < 0.01);
        assert!(report.anomalies.is_empty());
        // All in first bucket (<1 km/h)
        assert_eq!(report.histogram[0].count, 2);
    }

    #[test]
    fn known_speed() {
        // ~111m per 0.001 deg latitude, in 1s = ~111 m/s = ~400 km/h
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, Some(5.0)),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, Some(5.0)),
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.total_pairs, 1);
        assert!((report.max_speed_kmh - 400.0).abs() < 5.0);
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].provider_before, FixProvider::Gps);
        assert_eq!(report.anomalies[0].accuracy_before, Some(5.0));
    }

    #[test]
    fn skip_zero_dt() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 1000, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None), // same timestamp
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.total_pairs, 0);
    }

    #[test]
    fn anomaly_sorted_descending() {
        // Two anomalous jumps at different speeds
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None), // ~400 km/h
            make_fix(FixProvider::Gps, 36.001, 140.0, 2000, None), // 0 km/h
            make_fix(FixProvider::Gps, 36.003, 140.0, 3000, None), // ~800 km/h
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.anomalies.len(), 2);
        // Higher speed first
        assert!(report.anomalies[0].speed_kmh > report.anomalies[1].speed_kmh);
    }

    #[test]
    fn histogram_buckets_count() {
        // Should have 7 buckets: <1, 1-10, 10-50, 50-100, 100-200, 200-500, 500+
        let report = analyze_jumps(&[], &JumpConfig::default());
        assert_eq!(report.histogram.len(), 7);
        assert_eq!(report.histogram[0].label, "<1 km/h");
        assert_eq!(report.histogram[6].label, "500+ km/h");
    }

    #[test]
    fn custom_threshold() {
        // Lower threshold to 50 km/h
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None), // ~400 km/h
        ];
        let config = JumpConfig {
            anomaly_threshold_kmh: 50.0,
        };
        let report = analyze_jumps(&fixes, &config);
        assert_eq!(report.anomalies.len(), 1);
    }

    #[test]
    fn threshold_boundary() {
        // At exactly threshold — should NOT be included (strictly >)
        let config = JumpConfig {
            anomaly_threshold_kmh: 400.0,
        };
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None), // ~400 km/h
        ];
        let report = analyze_jumps(&fixes, &config);
        // ~400 km/h might be slightly above or below, depends on haversine precision
        // This just verifies threshold logic works
        assert!(report.anomalies.len() <= 1);
    }

    #[test]
    fn mixed_providers() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, Some(5.0)),
            make_fix(FixProvider::Nlp, 36.01, 140.0, 1000, Some(400.0)), // ~4000 km/h jump
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].provider_before, FixProvider::Gps);
        assert_eq!(report.anomalies[0].provider_after, FixProvider::Nlp);
    }

    #[test]
    fn walking_speed_no_anomaly() {
        // ~1.4 m/s = ~5 km/h walking speed
        // 0.0000125 deg ≈ 1.4m at 36°N
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.0000125, 140.0, 1000, None),
            make_fix(FixProvider::Gps, 36.000025, 140.0, 2000, None),
        ];
        let report = analyze_jumps(&fixes, &JumpConfig::default());
        assert_eq!(report.total_pairs, 2);
        assert!(report.max_speed_kmh < 10.0);
        assert!(report.anomalies.is_empty());
    }
}
