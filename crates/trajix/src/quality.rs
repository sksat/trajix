//! Fix quality classification.
//!
//! Classifies each fix as Primary (GPS/FLP), GapFallback (NLP during GPS/FLP gap),
//! or Rejected (NLP redundant with nearby GPS/FLP coverage).

use serde::{Deserialize, Serialize};

use crate::record::fix::FixRecord;
use crate::types::FixProvider;

/// Default gap threshold: if no GPS/FLP fix within 5 seconds, NLP is gap-fallback.
pub const DEFAULT_GAP_THRESHOLD_MS: i64 = 5000;

/// Quality classification for a fix record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixQuality {
    /// GPS or FLP fix — high-quality positioning source.
    Primary,
    /// NLP fix during a GPS/FLP coverage gap (useful fallback).
    GapFallback,
    /// NLP fix redundant with nearby GPS/FLP coverage (should be filtered).
    Rejected,
}

/// Streaming fix quality classifier.
///
/// Tracks GPS/FLP coverage and classifies each fix incrementally.
/// Use this when processing fixes one-by-one (e.g., in a streaming parser).
///
/// # Example
/// ```
/// use trajix::quality::{FixQualityClassifier, FixQuality};
/// use trajix::FixRecord;
///
/// let mut classifier = FixQualityClassifier::default();
/// // classifier.classify(&fix) returns FixQuality for each fix
/// ```
pub struct FixQualityClassifier {
    gap_threshold_ms: i64,
    last_gps_flp_time_ms: Option<i64>,
}

impl FixQualityClassifier {
    /// Create a new classifier with a custom gap threshold.
    pub fn new(gap_threshold_ms: i64) -> Self {
        Self {
            gap_threshold_ms,
            last_gps_flp_time_ms: None,
        }
    }

    /// Classify a single fix record.
    ///
    /// - GPS and FLP fixes are always `Primary`.
    /// - NLP fixes within `gap_threshold_ms` of the last GPS/FLP fix are `Rejected`.
    /// - NLP fixes beyond the gap threshold (or with no prior GPS/FLP) are `GapFallback`.
    pub fn classify(&mut self, fix: &FixRecord) -> FixQuality {
        match fix.provider {
            FixProvider::Gps | FixProvider::Flp => {
                self.last_gps_flp_time_ms = Some(fix.unix_time_ms);
                FixQuality::Primary
            }
            FixProvider::Nlp => match self.last_gps_flp_time_ms {
                Some(t) if (fix.unix_time_ms - t) <= self.gap_threshold_ms => FixQuality::Rejected,
                _ => FixQuality::GapFallback,
            },
        }
    }
}

impl Default for FixQualityClassifier {
    fn default() -> Self {
        Self::new(DEFAULT_GAP_THRESHOLD_MS)
    }
}

/// Classify fix quality for a sequence of fixes.
///
/// Fixes should be sorted by `unix_time_ms`. Returns a parallel `Vec<FixQuality>`
/// of the same length.
///
/// - GPS and FLP fixes are always `Primary`.
/// - NLP fixes within `gap_threshold_ms` of the last GPS/FLP fix are `Rejected`.
/// - NLP fixes beyond the gap threshold (or with no prior GPS/FLP) are `GapFallback`.
///
/// # Example
/// ```
/// use trajix::quality::{classify_fixes, FixQuality, DEFAULT_GAP_THRESHOLD_MS};
/// use trajix::FixRecord;
///
/// // Assuming you have parsed fixes...
/// let fixes: Vec<FixRecord> = vec![]; // your fixes here
/// let qualities = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
/// assert_eq!(qualities.len(), fixes.len());
/// ```
pub fn classify_fixes(fixes: &[FixRecord], gap_threshold_ms: i64) -> Vec<FixQuality> {
    let mut classifier = FixQualityClassifier::new(gap_threshold_ms);
    fixes.iter().map(|fix| classifier.classify(fix)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fix(provider: FixProvider, time_ms: i64) -> FixRecord {
        FixRecord {
            provider,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
            altitude_m: None,
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
    fn gps_is_primary() {
        let fixes = vec![make_fix(FixProvider::Gps, 1000)];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::Primary]);
    }

    #[test]
    fn flp_is_primary() {
        let fixes = vec![make_fix(FixProvider::Flp, 1000)];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::Primary]);
    }

    #[test]
    fn nlp_no_prior_gps_is_gap_fallback() {
        let fixes = vec![make_fix(FixProvider::Nlp, 1000)];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::GapFallback]);
    }

    #[test]
    fn nlp_within_gap_threshold_is_rejected() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),
            make_fix(FixProvider::Nlp, 3000), // 2s after GPS, within 5s
        ];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::Primary, FixQuality::Rejected]);
    }

    #[test]
    fn nlp_at_exact_threshold_is_rejected() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),
            make_fix(FixProvider::Nlp, 6000), // exactly 5s after GPS
        ];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::Primary, FixQuality::Rejected]);
    }

    #[test]
    fn nlp_beyond_gap_threshold_is_gap_fallback() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),
            make_fix(FixProvider::Nlp, 7000), // 6s after GPS, beyond 5s
        ];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(q, vec![FixQuality::Primary, FixQuality::GapFallback]);
    }

    #[test]
    fn mixed_sequence() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),  // Primary
            make_fix(FixProvider::Nlp, 2000),  // Rejected (1s after GPS)
            make_fix(FixProvider::Nlp, 8000),  // GapFallback (7s after GPS)
            make_fix(FixProvider::Flp, 9000),  // Primary
            make_fix(FixProvider::Nlp, 10000), // Rejected (1s after FLP)
        ];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert_eq!(
            q,
            vec![
                FixQuality::Primary,
                FixQuality::Rejected,
                FixQuality::GapFallback,
                FixQuality::Primary,
                FixQuality::Rejected,
            ]
        );
    }

    #[test]
    fn empty_input() {
        let q = classify_fixes(&[], DEFAULT_GAP_THRESHOLD_MS);
        assert!(q.is_empty());
    }

    #[test]
    fn custom_threshold() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),
            make_fix(FixProvider::Nlp, 3000), // 2s after GPS
        ];
        // With 1s threshold, NLP at 2s is a gap fallback
        let q = classify_fixes(&fixes, 1000);
        assert_eq!(q, vec![FixQuality::Primary, FixQuality::GapFallback]);
    }

    #[test]
    fn all_nlp_is_all_gap_fallback() {
        let fixes = vec![
            make_fix(FixProvider::Nlp, 1000),
            make_fix(FixProvider::Nlp, 2000),
            make_fix(FixProvider::Nlp, 3000),
        ];
        let q = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        assert!(q.iter().all(|&q| q == FixQuality::GapFallback));
    }

    // ── Streaming classifier tests ──

    #[test]
    fn streaming_matches_batch() {
        let fixes = vec![
            make_fix(FixProvider::Gps, 1000),
            make_fix(FixProvider::Nlp, 2000),
            make_fix(FixProvider::Nlp, 8000),
            make_fix(FixProvider::Flp, 9000),
            make_fix(FixProvider::Nlp, 10000),
        ];
        let batch = classify_fixes(&fixes, DEFAULT_GAP_THRESHOLD_MS);
        let mut classifier = FixQualityClassifier::default();
        let streaming: Vec<_> = fixes.iter().map(|f| classifier.classify(f)).collect();
        assert_eq!(batch, streaming);
    }

    #[test]
    fn streaming_default_threshold() {
        let mut c = FixQualityClassifier::default();
        // GPS at t=0, NLP at t=5000 (exactly at threshold) → Rejected
        assert_eq!(c.classify(&make_fix(FixProvider::Gps, 0)), FixQuality::Primary);
        assert_eq!(c.classify(&make_fix(FixProvider::Nlp, 5000)), FixQuality::Rejected);
        // NLP at t=5001 → GapFallback
        assert_eq!(c.classify(&make_fix(FixProvider::Nlp, 5001)), FixQuality::GapFallback);
    }

    #[test]
    fn streaming_tracks_state_across_calls() {
        let mut c = FixQualityClassifier::new(1000);
        assert_eq!(c.classify(&make_fix(FixProvider::Nlp, 100)), FixQuality::GapFallback);
        assert_eq!(c.classify(&make_fix(FixProvider::Gps, 200)), FixQuality::Primary);
        assert_eq!(c.classify(&make_fix(FixProvider::Nlp, 500)), FixQuality::Rejected);
        assert_eq!(c.classify(&make_fix(FixProvider::Nlp, 1500)), FixQuality::GapFallback);
    }
}
