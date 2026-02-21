//! Downsampling algorithms for high-frequency sensor data.
//!
//! Two strategies are provided:
//!
//! 1. **Temporal decimation** (`decimate_by_time`): Keep one sample per time interval.
//!    Simple, predictable, and safe. Use for reducing ~100Hz IMU data to ~10Hz.
//!
//! 2. **LTTB** (`lttb`): Largest Triangle Three Buckets algorithm for visual
//!    downsampling. Preserves peaks and shape of the signal. Use for chart rendering.

/// A timestamped data point for downsampling.
///
/// Generic over value type to support both single-channel and multi-channel data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample<V> {
    /// Timestamp in milliseconds (Unix epoch).
    pub time_ms: i64,
    /// The data value(s).
    pub value: V,
}

/// Decimate a sorted slice of samples by keeping at most one sample per `interval_ms`.
///
/// Within each interval, the sample closest to the interval boundary is selected.
/// The first and last samples are always preserved.
///
/// # Invariants
/// - Input must be sorted by `time_ms` (ascending).
/// - Output is a subset of input (no interpolation).
/// - Output preserves chronological order.
/// - Output always includes the first and last sample (if input is non-empty).
/// - Output length <= input length.
///
/// # Panics
/// Panics if `interval_ms <= 0`.
pub fn decimate_by_time<V: Copy>(samples: &[Sample<V>], interval_ms: i64) -> Vec<Sample<V>> {
    assert!(interval_ms > 0, "interval_ms must be positive");

    if samples.len() <= 2 {
        return samples.to_vec();
    }

    let mut result = Vec::new();

    // Always keep first sample
    result.push(samples[0]);
    let mut next_boundary = samples[0].time_ms + interval_ms;

    for &sample in &samples[1..samples.len() - 1] {
        if sample.time_ms >= next_boundary {
            result.push(sample);
            // Advance boundary from the sample we just kept (not from the theoretical boundary)
            // to prevent drift accumulation
            next_boundary = sample.time_ms + interval_ms;
        }
    }

    // Always keep last sample
    let last = samples[samples.len() - 1];
    // Avoid duplicate if last was already added
    if result.last().map(|s| s.time_ms) != Some(last.time_ms) {
        result.push(last);
    }

    result
}

/// LTTB (Largest Triangle Three Buckets) downsampling.
///
/// Reduces a time series to `target_count` points while preserving the visual shape.
/// Works by dividing data into buckets and selecting the point in each bucket that
/// forms the largest triangle with the selected points in adjacent buckets.
///
/// # Properties
/// - First and last points are always preserved.
/// - Output is a subset of input (no interpolation).
/// - Output preserves chronological order.
/// - Peaks, valleys, and sharp transitions are preferentially retained.
///
/// # Panics
/// Panics if `target_count < 2`.
///
/// # Reference
/// Sveinn Steinarsson, "Downsampling Time Series for Visual Representation", 2013.
pub fn lttb<V: LttbValue>(samples: &[Sample<V>], target_count: usize) -> Vec<Sample<V>> {
    assert!(target_count >= 2, "target_count must be >= 2");

    if samples.len() <= target_count {
        return samples.to_vec();
    }

    let n = samples.len();
    let mut result = Vec::with_capacity(target_count);

    // Always keep first point
    result.push(samples[0]);

    // Number of buckets between first and last (both are pre-selected)
    let bucket_count = target_count - 2;

    // Bucket size (floating point for even distribution)
    let bucket_size = (n - 2) as f64 / (bucket_count + 1) as f64;

    let mut prev_selected_idx = 0usize;

    for bucket_idx in 0..bucket_count {
        // Current bucket: indices in the range [bucket_start, bucket_end)
        let bucket_start = 1 + (bucket_idx as f64 * bucket_size) as usize;
        let bucket_end = 1 + ((bucket_idx + 1) as f64 * bucket_size) as usize;
        let bucket_end = bucket_end.min(n - 1);

        // Next bucket average (for triangle area calculation)
        let next_start = bucket_end;
        let next_end = if bucket_idx + 1 < bucket_count {
            1 + ((bucket_idx + 2) as f64 * bucket_size) as usize
        } else {
            n // last bucket extends to end
        };
        let next_end = next_end.min(n);

        let (avg_x, avg_y) = bucket_average(&samples[next_start..next_end]);

        // Find the point in current bucket that maximizes triangle area
        let prev_x = samples[prev_selected_idx].time_ms as f64;
        let prev_y = samples[prev_selected_idx].value.to_f64();

        let mut max_area = -1.0f64;
        let mut max_idx = bucket_start;

        for i in bucket_start..bucket_end {
            let area = triangle_area(
                prev_x,
                prev_y,
                samples[i].time_ms as f64,
                samples[i].value.to_f64(),
                avg_x,
                avg_y,
            );
            if area > max_area {
                max_area = area;
                max_idx = i;
            }
        }

        result.push(samples[max_idx]);
        prev_selected_idx = max_idx;
    }

    // Always keep last point
    result.push(samples[n - 1]);

    result
}

/// Trait for values that can be used with LTTB.
///
/// LTTB needs a single scalar value for triangle area computation.
/// For multi-channel data, compute the magnitude or select the primary channel.
pub trait LttbValue: Copy {
    fn to_f64(self) -> f64;
}

impl LttbValue for f64 {
    fn to_f64(self) -> f64 {
        self
    }
}

impl LttbValue for f32 {
    fn to_f64(self) -> f64 {
        self as f64
    }
}

fn triangle_area(x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> f64 {
    ((x1 * (y2 - y3) + x2 * (y3 - y1) + x3 * (y1 - y2)) / 2.0).abs()
}

fn bucket_average<V: LttbValue>(samples: &[Sample<V>]) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let n = samples.len() as f64;
    let sum_x: f64 = samples.iter().map(|s| s.time_ms as f64).sum();
    let sum_y: f64 = samples.iter().map(|s| s.value.to_f64()).sum();
    (sum_x / n, sum_y / n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_samples(data: &[(i64, f64)]) -> Vec<Sample<f64>> {
        data.iter()
            .map(|&(t, v)| Sample {
                time_ms: t,
                value: v,
            })
            .collect()
    }

    // ──────────────────────────────────────────────
    // decimate_by_time tests
    // ──────────────────────────────────────────────

    #[test]
    fn decimate_empty() {
        let result = decimate_by_time::<f64>(&[], 100);
        assert!(result.is_empty());
    }

    #[test]
    fn decimate_single() {
        let samples = make_samples(&[(1000, 1.0)]);
        let result = decimate_by_time(&samples, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time_ms, 1000);
    }

    #[test]
    fn decimate_two() {
        let samples = make_samples(&[(1000, 1.0), (1050, 2.0)]);
        let result = decimate_by_time(&samples, 100);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn decimate_100hz_to_10hz() {
        // Simulate 1 second of 100Hz data (100 samples, 10ms apart)
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: 1000 + i * 10,
                value: (i as f64 * 0.1).sin(),
            })
            .collect();

        let result = decimate_by_time(&samples, 100);

        // Should have roughly 10 samples + first + last
        // First (t=1000), then one per 100ms interval, plus last (t=1990)
        assert!(
            result.len() >= 10 && result.len() <= 12,
            "expected ~10 samples, got {}",
            result.len()
        );

        // First and last preserved
        assert_eq!(result.first().unwrap().time_ms, 1000);
        assert_eq!(result.last().unwrap().time_ms, 1990);
    }

    #[test]
    fn decimate_preserves_order() {
        let samples: Vec<_> = (0..50)
            .map(|i| Sample {
                time_ms: 1000 + i * 20,
                value: i as f64,
            })
            .collect();

        let result = decimate_by_time(&samples, 100);

        for window in result.windows(2) {
            assert!(
                window[0].time_ms < window[1].time_ms,
                "order violated: {} >= {}",
                window[0].time_ms,
                window[1].time_ms
            );
        }
    }

    #[test]
    fn decimate_output_is_subset() {
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: 1000 + i * 10,
                value: i as f64 * 0.5,
            })
            .collect();

        let result = decimate_by_time(&samples, 100);

        // Every output sample must exist in input
        for r in &result {
            assert!(
                samples.iter().any(|s| s.time_ms == r.time_ms && s.value == r.value),
                "output sample t={} v={} not found in input",
                r.time_ms,
                r.value
            );
        }
    }

    #[test]
    fn decimate_no_reduction_when_sparse() {
        // Samples already more than interval apart
        let samples = make_samples(&[(1000, 1.0), (2000, 2.0), (3000, 3.0)]);
        let result = decimate_by_time(&samples, 100);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn decimate_first_and_last_always_kept() {
        let samples: Vec<_> = (0..1000)
            .map(|i| Sample {
                time_ms: i,
                value: i as f64,
            })
            .collect();

        let result = decimate_by_time(&samples, 500);
        assert_eq!(result.first().unwrap().time_ms, 0);
        assert_eq!(result.last().unwrap().time_ms, 999);
    }

    #[test]
    #[should_panic(expected = "interval_ms must be positive")]
    fn decimate_panics_on_zero_interval() {
        decimate_by_time::<f64>(&[], 0);
    }

    #[test]
    fn decimate_large_interval_keeps_first_and_last() {
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: 1000 + i * 10,
                value: i as f64,
            })
            .collect();

        let result = decimate_by_time(&samples, 10000);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].time_ms, 1000);
        assert_eq!(result[1].time_ms, 1990);
    }

    // ──────────────────────────────────────────────
    // LTTB tests
    // ──────────────────────────────────────────────

    #[test]
    fn lttb_empty() {
        let result = lttb::<f64>(&[], 5);
        assert!(result.is_empty());
    }

    #[test]
    fn lttb_fewer_than_target() {
        let samples = make_samples(&[(1, 1.0), (2, 2.0), (3, 3.0)]);
        let result = lttb(&samples, 5);
        assert_eq!(result.len(), 3); // returns all
    }

    #[test]
    fn lttb_exact_target() {
        let samples = make_samples(&[(1, 1.0), (2, 2.0), (3, 3.0)]);
        let result = lttb(&samples, 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn lttb_preserves_first_and_last() {
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: i as i64,
                value: (i as f64 * 0.1).sin(),
            })
            .collect();

        let result = lttb(&samples, 10);
        assert_eq!(result.first().unwrap().time_ms, 0);
        assert_eq!(result.last().unwrap().time_ms, 99);
    }

    #[test]
    fn lttb_output_count() {
        let samples: Vec<_> = (0..1000)
            .map(|i| Sample {
                time_ms: i as i64,
                value: (i as f64 * 0.01).sin(),
            })
            .collect();

        let result = lttb(&samples, 50);
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn lttb_output_is_subset() {
        let samples: Vec<_> = (0..200)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (i as f64 * 0.05).sin(),
            })
            .collect();

        let result = lttb(&samples, 20);

        for r in &result {
            assert!(
                samples
                    .iter()
                    .any(|s| s.time_ms == r.time_ms && s.value == r.value),
                "output sample t={} v={} not found in input",
                r.time_ms,
                r.value
            );
        }
    }

    #[test]
    fn lttb_preserves_order() {
        let samples: Vec<_> = (0..200)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (i as f64 * 0.1).sin(),
            })
            .collect();

        let result = lttb(&samples, 20);

        for window in result.windows(2) {
            assert!(
                window[0].time_ms < window[1].time_ms,
                "order violated: {} >= {}",
                window[0].time_ms,
                window[1].time_ms
            );
        }
    }

    #[test]
    fn lttb_preserves_spike() {
        // Create mostly-flat data with one big spike
        let mut samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: i as i64,
                value: 0.0,
            })
            .collect();
        samples[50].value = 100.0; // spike

        let result = lttb(&samples, 10);

        // The spike should be preserved
        assert!(
            result.iter().any(|s| s.value == 100.0),
            "LTTB should preserve the spike"
        );
    }

    #[test]
    fn lttb_preserves_valley() {
        // Create data with a deep valley
        let samples: Vec<_> = (0..100)
            .map(|i| {
                let value = if i == 50 { -100.0 } else { 0.0 };
                Sample {
                    time_ms: i as i64,
                    value,
                }
            })
            .collect();

        let result = lttb(&samples, 10);

        assert!(
            result.iter().any(|s| s.value == -100.0),
            "LTTB should preserve the valley"
        );
    }

    #[test]
    #[should_panic(expected = "target_count must be >= 2")]
    fn lttb_panics_on_small_target() {
        lttb::<f64>(&[], 1);
    }

    #[test]
    fn lttb_sine_wave_preserves_extrema() {
        // Generate a sine wave - LTTB should preferentially keep peaks/valleys
        let samples: Vec<_> = (0..360)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (i as f64 * std::f64::consts::PI / 180.0).sin(),
            })
            .collect();

        let result = lttb(&samples, 20);

        // Check that max and min of result are close to 1.0 and -1.0
        let max_val = result.iter().map(|s| s.value).fold(f64::NEG_INFINITY, f64::max);
        let min_val = result.iter().map(|s| s.value).fold(f64::INFINITY, f64::min);

        assert!(
            max_val > 0.9,
            "LTTB should preserve near-peak: max was {max_val}"
        );
        assert!(
            min_val < -0.9,
            "LTTB should preserve near-valley: min was {min_val}"
        );
    }

    // ──────────────────────────────────────────────
    // triangle_area tests
    // ──────────────────────────────────────────────

    #[test]
    fn triangle_area_basic() {
        // Right triangle: (0,0), (1,0), (0,1) → area = 0.5
        let area = triangle_area(0.0, 0.0, 1.0, 0.0, 0.0, 1.0);
        assert!((area - 0.5).abs() < 1e-10);
    }

    #[test]
    fn triangle_area_collinear() {
        // Collinear points → area = 0
        let area = triangle_area(0.0, 0.0, 1.0, 1.0, 2.0, 2.0);
        assert!(area.abs() < 1e-10);
    }
}
