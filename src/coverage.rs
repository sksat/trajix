//! Coverage gap detection and NLP analysis.
//!
//! Detects gaps in GPS/FLP coverage and analyzes NLP fix timing
//! relative to those gaps.

use crate::record::fix::FixRecord;
use crate::stats::{PercentileStats, percentiles};
use crate::types::FixProvider;

/// Configuration for gap detection.
#[derive(Debug, Clone)]
pub struct GapConfig {
    /// Minimum gap duration in seconds to consider a coverage gap. Default: 5.0
    pub threshold_s: f64,
}

impl Default for GapConfig {
    fn default() -> Self {
        Self { threshold_s: 5.0 }
    }
}

impl GapConfig {
    /// Create from the quality module's gap threshold in milliseconds.
    pub fn from_threshold_ms(ms: i64) -> Self {
        Self {
            threshold_s: ms as f64 / 1000.0,
        }
    }
}

/// A single detected GPS/FLP coverage gap.
#[derive(Debug, Clone)]
pub struct CoverageGap {
    /// Gap start time (ms, Unix epoch).
    pub start_ms: i64,
    /// Gap end time (ms, Unix epoch).
    pub end_ms: i64,
    /// Gap duration in seconds.
    pub duration_s: f64,
}

/// NLP timing analysis relative to GPS/FLP gaps.
#[derive(Debug, Clone)]
pub struct NlpGapAnalysis {
    /// NLP fixes that fall within a GPS/FLP gap.
    pub nlp_in_gap: usize,
    /// NLP fixes that fall outside any GPS/FLP gap (redundant).
    pub nlp_outside_gap: usize,
    /// Accuracy percentile stats of NLP fixes during gaps.
    pub gap_nlp_accuracy_stats: Option<PercentileStats>,
}

/// Full coverage gap analysis result.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    /// Total time span in seconds (first to last fix).
    pub total_time_s: f64,
    /// Count of GPS+FLP fixes.
    pub gps_flp_count: usize,
    /// Count of NLP fixes.
    pub nlp_count: usize,
    /// All detected gaps, sorted by start time.
    pub gaps: Vec<CoverageGap>,
    /// Total gap time in seconds.
    pub total_gap_s: f64,
    /// Gap percentage of total session time.
    pub gap_percentage: f64,
    /// NLP timing analysis.
    pub nlp_analysis: NlpGapAnalysis,
}

/// Detect GPS/FLP coverage gaps and analyze NLP fix timing.
///
/// Fixes should be sorted by `unix_time_ms`.
pub fn analyze_coverage(fixes: &[FixRecord], config: &GapConfig) -> CoverageReport {
    if fixes.is_empty() {
        return CoverageReport {
            total_time_s: 0.0,
            gps_flp_count: 0,
            nlp_count: 0,
            gaps: Vec::new(),
            total_gap_s: 0.0,
            gap_percentage: 0.0,
            nlp_analysis: NlpGapAnalysis {
                nlp_in_gap: 0,
                nlp_outside_gap: 0,
                gap_nlp_accuracy_stats: None,
            },
        };
    }

    let t_start = fixes[0].unix_time_ms;
    let t_end = fixes[fixes.len() - 1].unix_time_ms;
    let total_time_s = (t_end - t_start) as f64 / 1000.0;

    let gps_flp: Vec<&FixRecord> = fixes
        .iter()
        .filter(|f| f.provider == FixProvider::Gps || f.provider == FixProvider::Flp)
        .collect();

    let nlp_only: Vec<&FixRecord> = fixes
        .iter()
        .filter(|f| f.provider == FixProvider::Nlp)
        .collect();

    let threshold_ms = (config.threshold_s * 1000.0) as i64;

    // Detect gaps
    let mut gaps: Vec<CoverageGap> = Vec::new();

    if !gps_flp.is_empty() {
        // Gap before first GPS/FLP fix
        if gps_flp[0].unix_time_ms - t_start > threshold_ms {
            let start = t_start;
            let end = gps_flp[0].unix_time_ms;
            gaps.push(CoverageGap {
                start_ms: start,
                end_ms: end,
                duration_s: (end - start) as f64 / 1000.0,
            });
        }

        // Gaps between consecutive GPS/FLP fixes
        for i in 1..gps_flp.len() {
            let dt_ms = gps_flp[i].unix_time_ms - gps_flp[i - 1].unix_time_ms;
            if dt_ms > threshold_ms {
                let start = gps_flp[i - 1].unix_time_ms;
                let end = gps_flp[i].unix_time_ms;
                gaps.push(CoverageGap {
                    start_ms: start,
                    end_ms: end,
                    duration_s: (end - start) as f64 / 1000.0,
                });
            }
        }

        // Gap after last GPS/FLP fix
        let last_gps_flp = gps_flp[gps_flp.len() - 1].unix_time_ms;
        if t_end - last_gps_flp > threshold_ms {
            gaps.push(CoverageGap {
                start_ms: last_gps_flp,
                end_ms: t_end,
                duration_s: (t_end - last_gps_flp) as f64 / 1000.0,
            });
        }
    } else {
        // No GPS/FLP at all — entire session is a gap
        if total_time_s > config.threshold_s {
            gaps.push(CoverageGap {
                start_ms: t_start,
                end_ms: t_end,
                duration_s: total_time_s,
            });
        }
    }

    let total_gap_s: f64 = gaps.iter().map(|g| g.duration_s).sum();
    let gap_percentage = if total_time_s > 0.0 {
        total_gap_s / total_time_s * 100.0
    } else {
        0.0
    };

    // NLP timing analysis
    let mut nlp_in_gap = 0usize;
    let mut nlp_outside_gap = 0usize;
    let mut gap_nlp_accs: Vec<f64> = Vec::new();

    for nlp in &nlp_only {
        let in_gap = gaps
            .iter()
            .any(|g| nlp.unix_time_ms >= g.start_ms && nlp.unix_time_ms <= g.end_ms);
        if in_gap {
            nlp_in_gap += 1;
            if let Some(acc) = nlp.accuracy_m {
                gap_nlp_accs.push(acc);
            }
        } else {
            nlp_outside_gap += 1;
        }
    }

    gap_nlp_accs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let gap_nlp_accuracy_stats = if gap_nlp_accs.is_empty() {
        None
    } else {
        Some(percentiles(&gap_nlp_accs))
    };

    CoverageReport {
        total_time_s,
        gps_flp_count: gps_flp.len(),
        nlp_count: nlp_only.len(),
        gaps,
        total_gap_s,
        gap_percentage,
        nlp_analysis: NlpGapAnalysis {
            nlp_in_gap,
            nlp_outside_gap,
            gap_nlp_accuracy_stats,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fix(provider: FixProvider, time_ms: i64, accuracy: Option<f64>) -> FixRecord {
        FixRecord {
            provider,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
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
        let report = analyze_coverage(&[], &GapConfig::default());
        assert_eq!(report.total_time_s, 0.0);
        assert_eq!(report.gps_flp_count, 0);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn single_gps_fix() {
        let fixes = vec![make_fix(FixProvider::Gps, 0, None)];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.total_time_s, 0.0);
        assert_eq!(report.gps_flp_count, 1);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn no_gap_close_fixes() {
        // GPS fixes 1s apart — no gap
        let fixes: Vec<FixRecord> = (0..10)
            .map(|i| make_fix(FixProvider::Gps, i * 1000, None))
            .collect();
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn single_gap_between_gps() {
        // GPS at 0s and 10s — 10s gap
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 10_000, None),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gaps.len(), 1);
        assert!((report.gaps[0].duration_s - 10.0).abs() < 0.01);
        assert_eq!(report.gaps[0].start_ms, 0);
        assert_eq!(report.gaps[0].end_ms, 10_000);
    }

    #[test]
    fn multiple_gaps() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 1_000, None),
            // 8s gap
            make_fix(FixProvider::Gps, 9_000, None),
            make_fix(FixProvider::Gps, 10_000, None),
            // 15s gap
            make_fix(FixProvider::Gps, 25_000, None),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gaps.len(), 2);
        assert!((report.gaps[0].duration_s - 8.0).abs() < 0.01);
        assert!((report.gaps[1].duration_s - 15.0).abs() < 0.01);
    }

    #[test]
    fn gap_at_start() {
        // NLP first, then GPS starts late
        let fixes = vec![
            make_fix(FixProvider::Nlp, 0, Some(400.0)),
            make_fix(FixProvider::Nlp, 3_000, Some(400.0)),
            make_fix(FixProvider::Gps, 8_000, Some(5.0)),
            make_fix(FixProvider::Gps, 9_000, Some(5.0)),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].start_ms, 0);
        assert_eq!(report.gaps[0].end_ms, 8_000);
    }

    #[test]
    fn gap_at_end() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 1_000, None),
            make_fix(FixProvider::Nlp, 8_000, Some(400.0)),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gaps.len(), 1);
        assert_eq!(report.gaps[0].start_ms, 1_000);
        assert_eq!(report.gaps[0].end_ms, 8_000);
    }

    #[test]
    fn nlp_only_session() {
        let fixes = vec![
            make_fix(FixProvider::Nlp, 0, Some(400.0)),
            make_fix(FixProvider::Nlp, 10_000, Some(400.0)),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gps_flp_count, 0);
        assert_eq!(report.nlp_count, 2);
        assert_eq!(report.gaps.len(), 1);
        assert!((report.gaps[0].duration_s - 10.0).abs() < 0.01);
    }

    #[test]
    fn nlp_in_gap_classification() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 1_000, None),
            // 9s gap
            make_fix(FixProvider::Nlp, 5_000, Some(100.0)), // in gap
            make_fix(FixProvider::Nlp, 6_000, Some(200.0)), // in gap
            make_fix(FixProvider::Gps, 10_000, None),
            make_fix(FixProvider::Nlp, 11_000, Some(50.0)), // outside gap
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.nlp_analysis.nlp_in_gap, 2);
        assert_eq!(report.nlp_analysis.nlp_outside_gap, 1);
        // Accuracy stats for in-gap NLP
        let acc = report.nlp_analysis.gap_nlp_accuracy_stats.as_ref().unwrap();
        assert_eq!(acc.min, 100.0);
        assert_eq!(acc.max, 200.0);
    }

    #[test]
    fn gap_percentage_calculation() {
        // 0-10s session, gap 5-10s = 50%
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 1_000, None),
            make_fix(FixProvider::Nlp, 10_000, None),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert!(report.gap_percentage > 0.0);
        // Gap is from 1000 to 10000 = 9s out of 10s = 90%
        assert!((report.gap_percentage - 90.0).abs() < 1.0);
    }

    #[test]
    fn custom_threshold() {
        // Gap of 8s, threshold 10s — no gap
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Gps, 8_000, None),
        ];
        let config = GapConfig { threshold_s: 10.0 };
        let report = analyze_coverage(&fixes, &config);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn from_threshold_ms() {
        let config = GapConfig::from_threshold_ms(5000);
        assert!((config.threshold_s - 5.0).abs() < 1e-10);
    }

    #[test]
    fn mixed_gps_flp() {
        // Both GPS and FLP count as primary
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Flp, 1_000, None),
            make_fix(FixProvider::Gps, 2_000, None),
            make_fix(FixProvider::Flp, 3_000, None),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.gps_flp_count, 4);
        assert!(report.gaps.is_empty());
    }

    #[test]
    fn nlp_no_accuracy_during_gap() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 0, None),
            make_fix(FixProvider::Nlp, 7_000, None), // in gap, no accuracy
            make_fix(FixProvider::Gps, 10_000, None),
        ];
        let report = analyze_coverage(&fixes, &GapConfig::default());
        assert_eq!(report.nlp_analysis.nlp_in_gap, 1);
        assert!(report.nlp_analysis.gap_nlp_accuracy_stats.is_none());
    }
}
