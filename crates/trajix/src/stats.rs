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

fn percentiles(sorted: &[f64]) -> PercentileStats {
    let n = sorted.len();
    PercentileStats {
        min: sorted[0],
        median: sorted[n / 2],
        p90: sorted[((n as f64 * 0.9) as usize).min(n - 1)],
        p95: sorted[((n as f64 * 0.95) as usize).min(n - 1)],
        max: sorted[n - 1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fix(provider: FixProvider, lat: f64, lon: f64, time_ms: i64, accuracy: Option<f64>) -> FixRecord {
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
            .map(|i| make_fix(FixProvider::Gps, 36.0, 140.0, i * 1000, Some(i as f64 + 1.0)))
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
}
