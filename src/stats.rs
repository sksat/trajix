//! Statistical analysis of fix records.
//!
//! Computes summary statistics for a set of fix records:
//! total distance, session duration, accuracy distribution, and per-provider counts.

use crate::geo;
use crate::record::fix::FixRecord;
use crate::types::FixProvider;

/// Summary statistics for a set of fix records.
#[derive(Debug, Clone)]
pub struct FixStats {
    /// Total number of fixes.
    pub count: usize,
    /// Session duration in seconds (last - first timestamp).
    pub duration_s: f64,
    /// Total haversine distance across consecutive fixes in meters.
    pub total_distance_m: f64,
    /// Horizontal accuracy percentiles (meters), if accuracy data is available.
    pub accuracy: Option<PercentileStats>,
    /// Number of fixes per provider.
    pub per_provider: Vec<ProviderCount>,
}

/// Percentile statistics for a distribution.
#[derive(Debug, Clone)]
pub struct PercentileStats {
    pub min: f64,
    pub median: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

/// Count of fixes per provider.
#[derive(Debug, Clone)]
pub struct ProviderCount {
    pub provider: FixProvider,
    pub count: usize,
}

/// Compute summary statistics for a slice of fix records.
///
/// Fixes should be sorted by `unix_time_ms` for meaningful distance and duration.
///
/// # Example
/// ```
/// use trajix::stats::summarize_fixes;
/// use trajix::FixRecord;
///
/// let fixes: Vec<FixRecord> = vec![]; // your fixes here
/// let stats = summarize_fixes(&fixes);
/// assert_eq!(stats.count, 0);
/// ```
pub fn summarize_fixes(fixes: &[FixRecord]) -> FixStats {
    if fixes.is_empty() {
        return FixStats {
            count: 0,
            duration_s: 0.0,
            total_distance_m: 0.0,
            accuracy: None,
            per_provider: Vec::new(),
        };
    }

    let count = fixes.len();

    // Duration
    let first_ms = fixes.first().unwrap().unix_time_ms;
    let last_ms = fixes.last().unwrap().unix_time_ms;
    let duration_s = (last_ms - first_ms) as f64 / 1000.0;

    // Total distance
    let total_distance_m = fixes
        .windows(2)
        .map(|w| {
            geo::haversine_distance_m(
                w[0].latitude_deg,
                w[0].longitude_deg,
                w[1].latitude_deg,
                w[1].longitude_deg,
            )
        })
        .sum();

    // Accuracy percentiles
    let mut accuracies: Vec<f64> = fixes.iter().filter_map(|f| f.accuracy_m).collect();
    accuracies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let accuracy = if accuracies.is_empty() {
        None
    } else {
        Some(percentiles(&accuracies))
    };

    // Per-provider counts
    let mut gps_count = 0usize;
    let mut flp_count = 0usize;
    let mut nlp_count = 0usize;
    for f in fixes {
        match f.provider {
            FixProvider::Gps => gps_count += 1,
            FixProvider::Flp => flp_count += 1,
            FixProvider::Nlp => nlp_count += 1,
        }
    }
    let mut per_provider = Vec::new();
    if gps_count > 0 {
        per_provider.push(ProviderCount {
            provider: FixProvider::Gps,
            count: gps_count,
        });
    }
    if flp_count > 0 {
        per_provider.push(ProviderCount {
            provider: FixProvider::Flp,
            count: flp_count,
        });
    }
    if nlp_count > 0 {
        per_provider.push(ProviderCount {
            provider: FixProvider::Nlp,
            count: nlp_count,
        });
    }

    FixStats {
        count,
        duration_s,
        total_distance_m,
        accuracy,
        per_provider,
    }
}

/// Compute percentile statistics from a pre-sorted slice of f64 values.
///
/// The input must be sorted in ascending order and non-empty.
///
/// # Panics
///
/// Panics if `sorted` is empty.
pub fn percentiles(sorted: &[f64]) -> PercentileStats {
    let n = sorted.len();
    PercentileStats {
        min: sorted[0],
        median: sorted[n / 2],
        p90: sorted[((n as f64 * 0.9) as usize).min(n - 1)],
        p95: sorted[((n as f64 * 0.95) as usize).min(n - 1)],
        p99: sorted[((n as f64 * 0.99) as usize).min(n - 1)],
        max: sorted[n - 1],
    }
}

/// Per-provider detailed statistics including accuracy distribution and missing field counts.
#[derive(Debug, Clone)]
pub struct ProviderDetailedStats {
    pub provider: FixProvider,
    pub count: usize,
    /// Accuracy percentiles (meters), if accuracy data is available.
    pub accuracy: Option<PercentileStats>,
    /// Count of fixes missing altitude.
    pub missing_altitude: usize,
    /// Count of fixes missing speed.
    pub missing_speed: usize,
    /// Count of fixes missing bearing.
    pub missing_bearing: usize,
}

/// Compute per-provider detailed statistics.
///
/// Returns one entry per distinct provider found, in order: GPS, FLP, NLP.
pub fn provider_detailed_stats(fixes: &[FixRecord]) -> Vec<ProviderDetailedStats> {
    let mut by_provider: std::collections::HashMap<FixProvider, Vec<&FixRecord>> =
        std::collections::HashMap::new();
    for f in fixes {
        by_provider.entry(f.provider).or_default().push(f);
    }

    let mut result = Vec::new();
    for provider in [FixProvider::Gps, FixProvider::Flp, FixProvider::Nlp] {
        let Some(pf) = by_provider.get(&provider) else {
            continue;
        };

        let mut accuracies: Vec<f64> = pf.iter().filter_map(|f| f.accuracy_m).collect();
        accuracies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let accuracy = if accuracies.is_empty() {
            None
        } else {
            Some(percentiles(&accuracies))
        };

        result.push(ProviderDetailedStats {
            provider,
            count: pf.len(),
            accuracy,
            missing_altitude: pf.iter().filter(|f| f.altitude_m.is_none()).count(),
            missing_speed: pf.iter().filter(|f| f.speed_mps.is_none()).count(),
            missing_bearing: pf.iter().filter(|f| f.bearing_deg.is_none()).count(),
        });
    }

    result
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
        let stats = summarize_fixes(&[]);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.duration_s, 0.0);
        assert_eq!(stats.total_distance_m, 0.0);
        assert!(stats.accuracy.is_none());
        assert!(stats.per_provider.is_empty());
    }

    #[test]
    fn single_fix() {
        let fixes = vec![make_fix(FixProvider::Gps, 36.0, 140.0, 1000, Some(5.0))];
        let stats = summarize_fixes(&fixes);
        assert_eq!(stats.count, 1);
        assert_eq!(stats.duration_s, 0.0);
        assert_eq!(stats.total_distance_m, 0.0);
        assert!(stats.accuracy.is_some());
        assert_eq!(stats.accuracy.unwrap().min, 5.0);
    }

    #[test]
    fn duration_calculated() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.0, 140.0, 5000, None),
        ];
        let stats = summarize_fixes(&fixes);
        assert!((stats.duration_s - 5.0).abs() < 1e-10);
    }

    #[test]
    fn total_distance() {
        // Two points ~111m apart (0.001 deg latitude)
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None),
        ];
        let stats = summarize_fixes(&fixes);
        assert!(
            (stats.total_distance_m - 111.0).abs() < 2.0,
            "expected ~111m, got {:.1}m",
            stats.total_distance_m
        );
    }

    #[test]
    fn total_distance_three_points() {
        // 3 points: each ~111m apart
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.001, 140.0, 1000, None),
            make_fix(FixProvider::Gps, 36.002, 140.0, 2000, None),
        ];
        let stats = summarize_fixes(&fixes);
        assert!(
            (stats.total_distance_m - 222.0).abs() < 4.0,
            "expected ~222m, got {:.1}m",
            stats.total_distance_m
        );
    }

    #[test]
    fn accuracy_percentiles() {
        let fixes: Vec<_> = (0..100)
            .map(|i| {
                make_fix(
                    FixProvider::Gps,
                    36.0,
                    140.0,
                    i * 1000,
                    Some(i as f64 + 1.0),
                )
            })
            .collect();
        let stats = summarize_fixes(&fixes);
        let acc = stats.accuracy.unwrap();
        assert_eq!(acc.min, 1.0);
        assert_eq!(acc.max, 100.0);
        assert_eq!(acc.median, 51.0); // index 50 of 100 elements
    }

    #[test]
    fn per_provider_counts() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Gps, 36.0, 140.0, 1000, None),
            make_fix(FixProvider::Flp, 36.0, 140.0, 2000, None),
            make_fix(FixProvider::Nlp, 36.0, 140.0, 3000, None),
        ];
        let stats = summarize_fixes(&fixes);
        assert_eq!(stats.per_provider.len(), 3);
        assert_eq!(stats.per_provider[0].provider, FixProvider::Gps);
        assert_eq!(stats.per_provider[0].count, 2);
        assert_eq!(stats.per_provider[1].provider, FixProvider::Flp);
        assert_eq!(stats.per_provider[1].count, 1);
        assert_eq!(stats.per_provider[2].provider, FixProvider::Nlp);
        assert_eq!(stats.per_provider[2].count, 1);
    }

    #[test]
    fn no_accuracy_data() {
        let fixes = vec![
            make_fix(FixProvider::Nlp, 36.0, 140.0, 0, None),
            make_fix(FixProvider::Nlp, 36.0, 140.0, 1000, None),
        ];
        let stats = summarize_fixes(&fixes);
        assert!(stats.accuracy.is_none());
    }

    // --- percentiles() tests ---

    #[test]
    fn percentiles_single_element() {
        let p = percentiles(&[42.0]);
        assert_eq!(p.min, 42.0);
        assert_eq!(p.median, 42.0);
        assert_eq!(p.p90, 42.0);
        assert_eq!(p.p95, 42.0);
        assert_eq!(p.p99, 42.0);
        assert_eq!(p.max, 42.0);
    }

    #[test]
    fn percentiles_100_elements() {
        let sorted: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        let p = percentiles(&sorted);
        assert_eq!(p.min, 1.0);
        assert_eq!(p.median, 51.0);
        assert_eq!(p.p90, 91.0);
        assert_eq!(p.p95, 96.0);
        assert_eq!(p.p99, 100.0);
        assert_eq!(p.max, 100.0);
    }

    #[test]
    fn percentiles_two_elements() {
        let p = percentiles(&[1.0, 10.0]);
        assert_eq!(p.min, 1.0);
        assert_eq!(p.max, 10.0);
        assert_eq!(p.median, 10.0); // index 1 of 2
    }

    // --- provider_detailed_stats() tests ---

    fn make_full_fix(
        provider: FixProvider,
        time_ms: i64,
        accuracy: Option<f64>,
        altitude: Option<f64>,
        speed: Option<f64>,
        bearing: Option<f64>,
    ) -> FixRecord {
        FixRecord {
            provider,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
            altitude_m: altitude,
            speed_mps: speed,
            accuracy_m: accuracy,
            bearing_deg: bearing,
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
    fn provider_detailed_stats_empty() {
        let result = provider_detailed_stats(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn provider_detailed_stats_single_provider() {
        let fixes = vec![
            make_full_fix(
                FixProvider::Gps,
                0,
                Some(5.0),
                Some(100.0),
                Some(1.0),
                Some(90.0),
            ),
            make_full_fix(FixProvider::Gps, 1000, Some(10.0), None, None, None),
        ];
        let result = provider_detailed_stats(&fixes);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].provider, FixProvider::Gps);
        assert_eq!(result[0].count, 2);
        assert!(result[0].accuracy.is_some());
        assert_eq!(result[0].accuracy.as_ref().unwrap().min, 5.0);
        assert_eq!(result[0].accuracy.as_ref().unwrap().max, 10.0);
        assert_eq!(result[0].missing_altitude, 1);
        assert_eq!(result[0].missing_speed, 1);
        assert_eq!(result[0].missing_bearing, 1);
    }

    #[test]
    fn provider_detailed_stats_multi_provider() {
        let fixes = vec![
            make_full_fix(
                FixProvider::Gps,
                0,
                Some(3.0),
                Some(100.0),
                Some(1.0),
                Some(0.0),
            ),
            make_full_fix(FixProvider::Flp, 1000, Some(15.0), Some(95.0), None, None),
            make_full_fix(FixProvider::Nlp, 2000, None, None, None, None),
        ];
        let result = provider_detailed_stats(&fixes);
        assert_eq!(result.len(), 3);
        // GPS
        assert_eq!(result[0].provider, FixProvider::Gps);
        assert_eq!(result[0].missing_altitude, 0);
        // FLP
        assert_eq!(result[1].provider, FixProvider::Flp);
        assert_eq!(result[1].missing_speed, 1);
        // NLP
        assert_eq!(result[2].provider, FixProvider::Nlp);
        assert!(result[2].accuracy.is_none());
        assert_eq!(result[2].missing_altitude, 1);
    }

    #[test]
    fn provider_detailed_stats_no_accuracy() {
        let fixes = vec![
            make_full_fix(FixProvider::Nlp, 0, None, None, None, None),
            make_full_fix(FixProvider::Nlp, 1000, None, None, None, None),
        ];
        let result = provider_detailed_stats(&fixes);
        assert_eq!(result.len(), 1);
        assert!(result[0].accuracy.is_none());
        assert_eq!(result[0].missing_altitude, 2);
    }
}
