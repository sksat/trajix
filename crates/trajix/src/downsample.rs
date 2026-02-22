//! Downsampling algorithms for high-frequency sensor data.
//!
//! Three strategies are provided:
//!
//! 1. **Temporal decimation** (`decimate_by_time`): Keep one sample per time interval.
//!    Simple, predictable, and safe. Use for reducing ~100Hz IMU data to ~10Hz.
//!
//! 2. **LTTB** (`lttb`): Largest Triangle Three Buckets algorithm for visual
//!    downsampling. Preserves peaks and shape of the signal. Use for chart rendering.
//!
//! 3. **Streaming decimation** (`StreamingDecimator`): Fixed-grid decimation that
//!    processes one sample at a time — no buffering of the full dataset. Ideal for
//!    inline decimation during parsing when the full dataset is too large to hold.

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

/// Decimate a sorted slice of samples by keeping one sample per fixed time bin.
///
/// Uses a **fixed grid** starting from the first sample's timestamp.
/// For each bin `[start + n*interval, start + (n+1)*interval)`, the sample closest
/// to the bin center is selected. This prevents drift that would occur if bins
/// were anchored to the previously selected sample.
///
/// The first and last samples are always preserved.
///
/// # Invariants
/// - Input must be sorted by `time_ms` (ascending).
/// - Output is a subset of input (no interpolation).
/// - Output preserves chronological order.
/// - Output always includes the first and last sample (if input is non-empty).
/// - Output length <= input length.
/// - Bins are uniformly spaced regardless of data distribution.
///
/// # Panics
/// Panics if `interval_ms <= 0`.
pub fn decimate_by_time<V: Copy>(samples: &[Sample<V>], interval_ms: i64) -> Vec<Sample<V>> {
    assert!(interval_ms > 0, "interval_ms must be positive");

    if samples.len() <= 2 {
        return samples.to_vec();
    }

    let start = samples[0].time_ms;
    let mut result = Vec::new();

    // Always keep first sample
    result.push(samples[0]);

    // Process bins on a fixed grid: [start + n*interval, start + (n+1)*interval)
    // Skip bin 0 (already covered by first sample). Start from bin 1.
    let mut bin_idx = 1i64;
    let mut i = 1; // current position in samples (skip first)
    let last_idx = samples.len() - 1;

    while i < last_idx {
        let bin_start = start + bin_idx * interval_ms;
        let bin_center = bin_start + interval_ms / 2;

        // Skip forward to the next bin if current sample is before it
        if samples[i].time_ms < bin_start {
            i += 1;
            continue;
        }

        // Find the sample closest to bin center within this bin
        let bin_end = bin_start + interval_ms;
        let mut best_idx = i;
        let mut best_dist = (samples[i].time_ms - bin_center).abs();

        let mut j = i + 1;
        while j < last_idx && samples[j].time_ms < bin_end {
            let dist = (samples[j].time_ms - bin_center).abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = j;
            }
            j += 1;
        }

        result.push(samples[best_idx]);
        i = j; // advance past this bin
        bin_idx += 1;

        // If we jumped multiple bins, catch up
        if i < last_idx {
            let next_bin_for_sample = (samples[i].time_ms - start) / interval_ms;
            if next_bin_for_sample > bin_idx {
                bin_idx = next_bin_for_sample;
            }
        }
    }

    // Always keep last sample
    let last = samples[last_idx];
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

/// LTTB that returns selected **indices** instead of values.
///
/// Use this for multi-axis data: run LTTB on one representative signal
/// (e.g., L2 magnitude) and use the returned indices to select the same
/// samples from all axes, keeping them aligned.
///
/// # Example
/// ```
/// use trajix::downsample::{lttb_indices, Sample};
///
/// let magnitude: Vec<Sample<f64>> = vec![/* ... */];
/// let indices = lttb_indices(&magnitude, 50);
/// // Use indices to select from x, y, z arrays
/// ```
pub fn lttb_indices<V: LttbValue>(samples: &[Sample<V>], target_count: usize) -> Vec<usize> {
    assert!(target_count >= 2, "target_count must be >= 2");

    if samples.len() <= target_count {
        return (0..samples.len()).collect();
    }

    let n = samples.len();
    let mut indices = Vec::with_capacity(target_count);

    indices.push(0);

    let bucket_count = target_count - 2;
    let bucket_size = (n - 2) as f64 / (bucket_count + 1) as f64;
    let mut prev_selected_idx = 0usize;

    for bucket_idx in 0..bucket_count {
        let bucket_start = 1 + (bucket_idx as f64 * bucket_size) as usize;
        let bucket_end = (1 + ((bucket_idx + 1) as f64 * bucket_size) as usize).min(n - 1);

        let next_start = bucket_end;
        let next_end = if bucket_idx + 1 < bucket_count {
            1 + ((bucket_idx + 2) as f64 * bucket_size) as usize
        } else {
            n
        };
        let next_end = next_end.min(n);

        let (avg_x, avg_y) = bucket_average(&samples[next_start..next_end]);

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

        indices.push(max_idx);
        prev_selected_idx = max_idx;
    }

    indices.push(n - 1);
    indices
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

// ──────────────────────────────────────────────
// Streaming decimator
// ──────────────────────────────────────────────

/// A timestamped sample output by [`StreamingDecimator`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DecimatedSample<V: Clone> {
    pub time_ms: i64,
    pub value: V,
}

/// Fixed-grid streaming decimator.
///
/// Keeps one sample per time bin (closest to bin center).
/// Operates on one sample at a time — no buffering of the full dataset.
/// Ideal for inline decimation during parsing.
///
/// # Example
/// ```
/// use trajix::downsample::StreamingDecimator;
///
/// // Decimate 100Hz accelerometer data to 10Hz
/// let mut d = StreamingDecimator::new(100); // 100ms bins
/// for i in 0..100 {
///     d.push(1000 + i * 10, i as f64); // 10ms apart = 100Hz
/// }
/// let result = d.finalize();
/// assert!(result.len() >= 10 && result.len() <= 12);
/// ```
pub struct StreamingDecimator<V: Clone> {
    interval_ms: i64,
    /// Grid origin (first sample's time).
    grid_start: Option<i64>,
    /// Current bin index.
    current_bin: i64,
    /// Best candidate in current bin.
    best: Option<(i64, V)>,
    /// Best candidate's distance to bin center.
    best_dist: i64,
    /// Output buffer.
    output: Vec<DecimatedSample<V>>,
}

impl<V: Clone> StreamingDecimator<V> {
    /// Create a new streaming decimator with the given bin interval in milliseconds.
    pub fn new(interval_ms: i64) -> Self {
        Self {
            interval_ms,
            grid_start: None,
            current_bin: 0,
            best: None,
            best_dist: i64::MAX,
            output: Vec::new(),
        }
    }

    /// Feed a single sample into the decimator.
    pub fn push(&mut self, time_ms: i64, value: V) {
        let start = match self.grid_start {
            Some(s) => s,
            None => {
                // First sample: emit it and set grid origin
                self.grid_start = Some(time_ms);
                self.current_bin = 0;
                self.output.push(DecimatedSample { time_ms, value });
                return;
            }
        };

        let bin = (time_ms - start).div_euclid(self.interval_ms);

        if bin > self.current_bin {
            // Flush previous bin's best candidate
            self.flush_best();
            self.current_bin = bin;
        }

        // Check if this sample is closer to current bin center
        let center = start + bin * self.interval_ms + self.interval_ms / 2;
        let dist = (time_ms - center).abs();
        if dist < self.best_dist {
            self.best_dist = dist;
            self.best = Some((time_ms, value));
        }
    }

    fn flush_best(&mut self) {
        if let Some((time_ms, value)) = self.best.take() {
            self.output.push(DecimatedSample { time_ms, value });
        }
        self.best_dist = i64::MAX;
    }

    /// Finalize the decimator and return all selected samples.
    pub fn finalize(mut self) -> Vec<DecimatedSample<V>> {
        self.flush_best();
        self.output
    }

    /// Number of samples that would be output (including unflushed best).
    pub fn output_count(&self) -> usize {
        self.output.len() + if self.best.is_some() { 1 } else { 0 }
    }
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

    // ──────────────────────────────────────────────
    // lttb_indices tests (multi-axis support)
    // ──────────────────────────────────────────────

    #[test]
    fn lttb_indices_returns_correct_count() {
        let samples: Vec<_> = (0..200)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (i as f64 * 0.05).sin(),
            })
            .collect();

        let indices = lttb_indices(&samples, 20);
        assert_eq!(indices.len(), 20);
    }

    #[test]
    fn lttb_indices_first_and_last() {
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: i as i64,
                value: i as f64,
            })
            .collect();

        let indices = lttb_indices(&samples, 10);
        assert_eq!(indices[0], 0);
        assert_eq!(*indices.last().unwrap(), 99);
    }

    #[test]
    fn lttb_indices_match_lttb_values() {
        let samples: Vec<_> = (0..200)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (i as f64 * 0.1).sin(),
            })
            .collect();

        let values = lttb(&samples, 15);
        let indices = lttb_indices(&samples, 15);

        assert_eq!(values.len(), indices.len());
        for (val, &idx) in values.iter().zip(indices.iter()) {
            assert_eq!(val.time_ms, samples[idx].time_ms);
            assert_eq!(val.value, samples[idx].value);
        }
    }

    #[test]
    fn lttb_indices_multi_axis_alignment() {
        // Simulate 3-axis accelerometer: compute magnitude for LTTB,
        // then use indices to select from all axes
        let n = 200;
        let x: Vec<f64> = (0..n).map(|i| (i as f64 * 0.1).sin()).collect();
        let y: Vec<f64> = (0..n).map(|i| (i as f64 * 0.1).cos()).collect();
        let z: Vec<f64> = (0..n).map(|i| (i as f64 * 0.05).sin()).collect();

        let magnitude: Vec<Sample<f64>> = (0..n)
            .map(|i| Sample {
                time_ms: i as i64 * 10,
                value: (x[i] * x[i] + y[i] * y[i] + z[i] * z[i]).sqrt(),
            })
            .collect();

        let indices = lttb_indices(&magnitude, 20);
        assert_eq!(indices.len(), 20);

        // All indices should be valid and in bounds
        for &idx in &indices {
            assert!(idx < n);
        }

        // Indices should be in ascending order
        for window in indices.windows(2) {
            assert!(window[0] < window[1]);
        }

        // Using indices to select from all axes gives aligned data
        let selected_x: Vec<f64> = indices.iter().map(|&i| x[i]).collect();
        let selected_y: Vec<f64> = indices.iter().map(|&i| y[i]).collect();
        let selected_z: Vec<f64> = indices.iter().map(|&i| z[i]).collect();

        assert_eq!(selected_x.len(), 20);
        assert_eq!(selected_y.len(), 20);
        assert_eq!(selected_z.len(), 20);
    }

    // ──────────────────────────────────────────────
    // Edge case tests (from Codex review)
    // ──────────────────────────────────────────────

    #[test]
    fn decimate_fixed_grid_no_drift() {
        // With fixed grid, bins are uniformly spaced regardless of data.
        // Create data at slightly irregular intervals and verify bins are stable.
        let mut samples = Vec::new();
        let mut t = 1000i64;
        for i in 0..100 {
            samples.push(Sample {
                time_ms: t,
                value: i as f64,
            });
            // Irregular spacing: 8-12ms
            t += 10 + (i % 3) as i64 - 1;
        }

        let result = decimate_by_time(&samples, 100);

        // Verify bins don't drift: consecutive selected points should be
        // roughly interval_ms apart (within one bin width)
        for window in result[1..result.len() - 1].windows(2) {
            let gap = window[1].time_ms - window[0].time_ms;
            assert!(
                gap >= 80 && gap <= 200,
                "unexpected gap {gap} between t={} and t={}",
                window[0].time_ms,
                window[1].time_ms
            );
        }
    }

    #[test]
    fn decimate_with_time_gap() {
        // Data with a big gap in the middle (simulating sensor dropout)
        let mut samples: Vec<Sample<f64>> = Vec::new();
        // First burst: 0-500ms at 10ms intervals
        for i in 0..50 {
            samples.push(Sample {
                time_ms: i * 10,
                value: i as f64,
            });
        }
        // Gap: 5 seconds
        // Second burst: 5500-6000ms at 10ms intervals
        for i in 0..50 {
            samples.push(Sample {
                time_ms: 5500 + i * 10,
                value: (50 + i) as f64,
            });
        }

        let result = decimate_by_time(&samples, 100);

        // Should have samples from both bursts
        let has_early = result.iter().any(|s| s.time_ms < 1000);
        let has_late = result.iter().any(|s| s.time_ms > 5000);
        assert!(has_early, "should have samples from first burst");
        assert!(has_late, "should have samples from second burst");

        // First and last preserved
        assert_eq!(result.first().unwrap().time_ms, 0);
        assert_eq!(result.last().unwrap().time_ms, 5990);
    }

    #[test]
    fn decimate_selects_nearest_to_center() {
        // Fixed grid: bins are [1000,1100), [1100,1200), [1200,1300), ...
        // Bin 1 = [1100, 1200), center at 1150
        // Place samples at 1110, 1148, 1190 → 1148 is closest to center 1150
        let samples = make_samples(&[
            (1000, 0.0),  // first (always kept, bin 0)
            (1110, 1.0),  // bin 1, distance to 1150 = 40
            (1148, 2.0),  // bin 1, distance to 1150 = 2 (closest)
            (1190, 3.0),  // bin 1, distance to 1150 = 40
            (2000, 4.0),  // last (always kept)
        ]);

        let result = decimate_by_time(&samples, 100);

        // Should keep first, one from bin 1, and last
        assert_eq!(result.first().unwrap().time_ms, 1000);
        assert_eq!(result.last().unwrap().time_ms, 2000);

        // The selected point from bin 1 should be closest to center (1150)
        let bin_point = result.iter().find(|s| s.time_ms > 1000 && s.time_ms < 2000);
        assert!(bin_point.is_some(), "should have a point from bin 1");
        assert_eq!(bin_point.unwrap().time_ms, 1148);
    }

    #[test]
    fn lttb_constant_data() {
        // All values the same - should still work without panics
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: i as i64,
                value: 42.0,
            })
            .collect();

        let result = lttb(&samples, 10);
        assert_eq!(result.len(), 10);
        assert!(result.iter().all(|s| s.value == 42.0));
    }

    #[test]
    fn lttb_two_distinct_values() {
        // Step function: 0 then 1
        let samples: Vec<_> = (0..100)
            .map(|i| Sample {
                time_ms: i as i64,
                value: if i < 50 { 0.0 } else { 1.0 },
            })
            .collect();

        let result = lttb(&samples, 10);
        assert_eq!(result.len(), 10);

        // Should have both 0.0 and 1.0 values
        assert!(result.iter().any(|s| s.value == 0.0));
        assert!(result.iter().any(|s| s.value == 1.0));
    }

    // ──────────────────────────────────────────────
    // StreamingDecimator tests
    // ──────────────────────────────────────────────

    #[test]
    fn streaming_decimator_basic() {
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
    fn streaming_decimator_sparse_input() {
        let mut d = StreamingDecimator::new(100);
        d.push(1000, 1.0);
        d.push(2000, 2.0);
        d.push(3000, 3.0);
        let result = d.finalize();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn streaming_decimator_single_sample() {
        let mut d = StreamingDecimator::new(100);
        d.push(1000, 42.0);
        let result = d.finalize();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].time_ms, 1000);
    }

    #[test]
    fn streaming_decimator_empty() {
        let d: StreamingDecimator<f64> = StreamingDecimator::new(100);
        let result = d.finalize();
        assert!(result.is_empty());
    }

    #[test]
    fn streaming_decimator_selects_nearest_to_center() {
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
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].time_ms, 1000);
        assert_eq!(result[1].time_ms, 1148);
    }

    #[test]
    fn streaming_decimator_preserves_order() {
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

    #[test]
    fn streaming_decimator_output_count() {
        let mut d = StreamingDecimator::new(100);
        for i in 0..100 {
            d.push(1000 + i * 10, i as f64);
        }
        let count = d.output_count();
        let result = d.finalize();
        assert_eq!(count, result.len());
    }
}
