//! IMU-based Dead Reckoning with GNSS fusion.
//!
//! Estimates position using accelerometer and attitude data when GNSS
//! signals are degraded. Fuses with GNSS fixes when available.
//!
//! # Algorithm
//!
//! 1. Receive quaternion attitude from `GameRotationVector` sensor
//! 2. Transform accelerometer readings from device frame to world frame (ENU)
//! 3. Remove gravity component from world-frame acceleration
//! 4. Integrate acceleration → velocity → position (semi-implicit Euler)
//! 5. Convert local ENU position to lat/lon using the GNSS anchor point
//!
//! DR activates when GNSS accuracy exceeds a configurable threshold and
//! deactivates when a good GNSS fix arrives.

use nalgebra::{Quaternion, UnitQuaternion, Vector3};

use crate::parser::line::Record;
use crate::record::fix::FixRecord;
use crate::record::sensor::{GameRotationVectorRecord, UncalibratedSensorRecord};

/// Standard gravity (m/s²).
const GRAVITY_MS2: f64 = 9.80665;

/// Meters per degree of latitude (approximate, WGS84 mean).
const METERS_PER_DEG_LAT: f64 = 111_132.0;

// ────────────────────────────────────────────
// Timestamp newtype
// ────────────────────────────────────────────

/// Timestamp in wall-clock milliseconds (UTC epoch).
///
/// All inputs to [`DeadReckoning`] must use the same time base.
/// For GNSS Logger data, both Fix (`unix_time_ms`) and sensor
/// (`utc_time_ms`) records use wall-clock milliseconds since Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TimestampMs(pub i64);

impl TimestampMs {
    /// Time delta in seconds (for numerical integration).
    pub fn dt_seconds(self, earlier: Self) -> f64 {
        (self.0 - earlier.0) as f64 / 1000.0
    }

    /// Time delta in milliseconds (for duration comparisons).
    pub fn elapsed_ms(self, earlier: Self) -> i64 {
        self.0 - earlier.0
    }

    /// Raw millisecond value.
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for TimestampMs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}ms", self.0)
    }
}

// ────────────────────────────────────────────
// Output types
// ────────────────────────────────────────────

/// Source of a trajectory point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointSource {
    /// Position from a GNSS fix (GPS, FLP, etc.).
    Gnss,
    /// Position estimated by dead reckoning (IMU integration from last GNSS anchor).
    DeadReckoning,
}

/// A single trajectory point (GNSS or dead-reckoned).
#[derive(Debug, Clone)]
pub struct TrajectoryPoint {
    pub time_ms: TimestampMs,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub source: PointSource,
}

/// Post-processing smoothing method for dead-reckoned segments.
///
/// Applied after [`DeadReckoning::finalize`] via [`smooth_trajectory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothingMethod {
    /// Replace DR points with straight line between bounding GNSS fixes.
    Linear,
    /// Keep DR trajectory shape, distribute endpoint error with t² weighting.
    /// Preserves turns and shape from IMU while correcting drift.
    EndpointConstrained,
}

// ────────────────────────────────────────────
// Input types (parser-independent)
// ────────────────────────────────────────────

/// A GNSS position fix for dead reckoning input.
///
/// Parser-independent: construct directly or via `From<&FixRecord>`.
#[derive(Debug, Clone)]
pub struct GnssFix {
    pub time_ms: TimestampMs,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub accuracy_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub bearing_deg: Option<f64>,
}

/// An IMU accelerometer sample for dead reckoning input.
///
/// Acceleration should be calibrated (bias removed), in device frame, m/s².
/// Includes gravity (i.e., "proper acceleration": stationary phone reads +9.8 on Z).
///
/// Parser-independent: construct directly or via `From<&UncalibratedSensorRecord>`.
#[derive(Debug, Clone)]
pub struct ImuSample {
    pub time_ms: TimestampMs,
    /// Calibrated acceleration in device frame \[x, y, z\] (m/s²).
    pub accel: [f64; 3],
}

impl ImuSample {
    /// Construct from uncalibrated raw acceleration and estimated bias.
    ///
    /// Computes `accel = raw - bias` for each axis.
    pub fn from_uncalibrated(time_ms: TimestampMs, raw: [f64; 3], bias: [f64; 3]) -> Self {
        Self {
            time_ms,
            accel: [raw[0] - bias[0], raw[1] - bias[1], raw[2] - bias[2]],
        }
    }
}

/// Device attitude as a quaternion for dead reckoning input.
///
/// Uses the Android `GameRotationVector` convention: quaternion components
/// (x, y, z, w) representing the rotation from ENU (East-North-Up) world
/// frame to the device frame. The conjugate rotates device → ENU.
///
/// Parser-independent: construct directly or via `From<&GameRotationVectorRecord>`.
#[derive(Debug, Clone)]
pub struct AttitudeSample {
    pub time_ms: TimestampMs,
    pub quaternion: DeviceQuaternion,
}

/// Device orientation quaternion (Android convention).
///
/// Components follow the Android sensor convention: (x, y, z, w)
/// where the quaternion represents the rotation from ENU world frame
/// to the device frame.
#[derive(Debug, Clone, Copy)]
pub struct DeviceQuaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl DeviceQuaternion {
    /// Convert to nalgebra `UnitQuaternion`, mapping Android (x,y,z,w) → nalgebra (w,x,y,z).
    pub(crate) fn to_unit_quaternion(self) -> UnitQuaternion<f64> {
        UnitQuaternion::new_normalize(Quaternion::new(self.w, self.x, self.y, self.z))
    }
}

// ────────────────────────────────────────────
// Configuration
// ────────────────────────────────────────────

/// Numerical integration method for dead reckoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum IntegrationMethod {
    /// Semi-implicit Euler: update velocity first (v += a·dt), then
    /// position (p += v·dt). Simple, symplectic, and adequate for
    /// short DR segments (typically < 2 minutes).
    #[default]
    SemiImplicitEuler,
}

/// Configuration for the Dead Reckoning processor.
#[derive(Debug, Clone)]
pub struct DeadReckoningConfig {
    /// Numerical integration method.
    pub integration: IntegrationMethod,
    /// GNSS accuracy threshold (meters).
    /// Fixes with `accuracy_m > threshold` are considered degraded.
    pub accuracy_threshold_m: f64,
    /// Speed threshold for Zero Velocity Update (m/s).
    /// When estimated velocity magnitude drops below this, reset to zero.
    pub zupt_speed_threshold_mps: f64,
    /// Maximum DR segment duration (milliseconds).
    pub max_dr_duration_ms: i64,
    /// Minimum time step for integration (seconds). Samples closer are skipped.
    pub min_dt_s: f64,
    /// Maximum time step (seconds). Gaps larger than this reset velocity.
    pub max_dt_s: f64,
    /// Maximum age of attitude data (milliseconds) for IMU integration.
    /// If the last attitude sample is older than this when `push_imu` is called,
    /// the IMU sample is rejected (returns `None`).
    /// `None` disables the staleness check. Default: `Some(500)`.
    pub max_attitude_age_ms: Option<i64>,
}

impl Default for DeadReckoningConfig {
    fn default() -> Self {
        Self {
            integration: IntegrationMethod::default(),
            accuracy_threshold_m: 30.0,
            zupt_speed_threshold_mps: 0.3,
            max_dr_duration_ms: 120_000,
            min_dt_s: 0.001,
            max_dt_s: 0.5,
            max_attitude_age_ms: Some(500),
        }
    }
}

// ────────────────────────────────────────────
// From impls: parser types → input types
// ────────────────────────────────────────────

impl From<&FixRecord> for GnssFix {
    fn from(f: &FixRecord) -> Self {
        Self {
            time_ms: TimestampMs(f.unix_time_ms),
            latitude_deg: f.latitude_deg,
            longitude_deg: f.longitude_deg,
            altitude_m: f.altitude_m.unwrap_or(0.0),
            accuracy_m: f.accuracy_m,
            speed_mps: f.speed_mps,
            bearing_deg: f.bearing_deg,
        }
    }
}

impl From<&UncalibratedSensorRecord> for ImuSample {
    fn from(s: &UncalibratedSensorRecord) -> Self {
        Self {
            time_ms: TimestampMs(s.utc_time_ms),
            accel: [s.x - s.bias_x, s.y - s.bias_y, s.z - s.bias_z],
        }
    }
}

impl From<&GameRotationVectorRecord> for AttitudeSample {
    fn from(g: &GameRotationVectorRecord) -> Self {
        Self {
            time_ms: TimestampMs(g.utc_time_ms),
            quaternion: DeviceQuaternion {
                x: g.x,
                y: g.y,
                z: g.z,
                w: g.w,
            },
        }
    }
}

// ────────────────────────────────────────────
// ────────────────────────────────────────────
// Internal state
// ────────────────────────────────────────────

struct GnssAnchor {
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,
    speed_mps: Option<f64>,
    bearing_deg: Option<f64>,
}

struct DrState {
    time_ms: TimestampMs,
    pos_enu: Vector3<f64>,
    vel_enu: Vector3<f64>,
    anchor_lat_deg: f64,
    anchor_lon_deg: f64,
    anchor_alt_m: f64,
    dr_start_ms: TimestampMs,
}

// ────────────────────────────────────────────
// Diagnostics
// ────────────────────────────────────────────

/// Diagnostic counters for Dead Reckoning processing.
#[derive(Debug, Clone, Default)]
pub struct DrDiagnostics {
    /// Total IMU samples received.
    pub imu_total: u64,
    /// IMU rejected: no attitude available.
    pub imu_rejected_no_attitude: u64,
    /// IMU rejected: no DR state (no prior GNSS fix pair).
    pub imu_rejected_no_state: u64,
    /// IMU rejected: attitude data too old.
    pub imu_rejected_stale_attitude: u64,
    /// IMU rejected: DR segment exceeded max duration.
    pub imu_rejected_max_duration: u64,
    /// IMU rejected: time step too small (< min_dt_s).
    pub imu_rejected_min_dt: u64,
    /// IMU rejected: time step too large (> max_dt_s), velocity reset.
    pub imu_rejected_gap: u64,
    /// IMU samples successfully integrated.
    pub imu_integrated: u64,
    /// Total attitude samples received.
    pub attitude_total: u64,
    /// Total GNSS fixes received.
    pub gnss_total: u64,
    /// GNSS fixes emitted as trajectory points (good accuracy).
    pub gnss_emitted: u64,
}

// ────────────────────────────────────────────
// DeadReckoning processor
// ────────────────────────────────────────────

/// Streaming Dead Reckoning processor.
///
/// Feed sensor data in chronological order via [`Self::push_gnss`], [`Self::push_imu`],
/// [`Self::push_attitude`], or dispatch parsed records via [`Self::push_record`], then
/// call [`Self::finalize`] to get the merged trajectory.
pub struct DeadReckoning {
    config: DeadReckoningConfig,
    last_fix: Option<GnssAnchor>,
    state: Option<DrState>,
    attitude: Option<(UnitQuaternion<f64>, TimestampMs)>,
    trajectory: Vec<TrajectoryPoint>,
    diag: DrDiagnostics,
}

impl DeadReckoning {
    pub fn new(config: DeadReckoningConfig) -> Self {
        Self {
            config,
            last_fix: None,
            state: None,
            attitude: None,
            trajectory: Vec::new(),
            diag: DrDiagnostics::default(),
        }
    }

    /// Return diagnostic counters.
    pub fn diagnostics(&self) -> &DrDiagnostics {
        &self.diag
    }

    // ── Parser-independent API ──

    /// Process a GNSS fix.
    ///
    /// Returns `Some(TrajectoryPoint)` with `source: Gnss` when the fix has good accuracy.
    /// Returns `None` for degraded fixes (which start or continue DR internally).
    pub fn push_gnss(&mut self, fix: &GnssFix) -> Option<TrajectoryPoint> {
        self.diag.gnss_total += 1;
        let accuracy = fix.accuracy_m.unwrap_or(f64::MAX);

        if accuracy <= self.config.accuracy_threshold_m {
            self.diag.gnss_emitted += 1;
            // Good fix: end DR, update anchor, emit point
            self.state = None;
            self.last_fix = Some(GnssAnchor {
                lat_deg: fix.latitude_deg,
                lon_deg: fix.longitude_deg,
                alt_m: fix.altitude_m,
                speed_mps: fix.speed_mps,
                bearing_deg: fix.bearing_deg,
            });
            let point = TrajectoryPoint {
                time_ms: fix.time_ms,
                latitude_deg: fix.latitude_deg,
                longitude_deg: fix.longitude_deg,
                altitude_m: fix.altitude_m,
                source: PointSource::Gnss,
            };
            self.trajectory.push(point.clone());
            Some(point)
        } else {
            if self.state.is_none() {
                // Degraded fix: start DR from last good anchor
                if let Some(anchor) = &self.last_fix {
                    let vel = velocity_from_anchor(anchor);
                    self.state = Some(DrState {
                        time_ms: fix.time_ms,
                        pos_enu: Vector3::zeros(),
                        vel_enu: vel,
                        anchor_lat_deg: anchor.lat_deg,
                        anchor_lon_deg: anchor.lon_deg,
                        anchor_alt_m: anchor.alt_m,
                        dr_start_ms: fix.time_ms,
                    });
                }
            }
            None
        }
    }

    /// Process an IMU accelerometer sample.
    ///
    /// Returns `Some(TrajectoryPoint)` with `source: DeadReckoning` when DR is active
    /// and a new position estimate is produced.
    pub fn push_imu(&mut self, sample: &ImuSample) -> Option<TrajectoryPoint> {
        self.diag.imu_total += 1;

        let (attitude, attitude_time) = match self.attitude {
            Some(a) => a,
            None => {
                self.diag.imu_rejected_no_attitude += 1;
                return None;
            }
        };
        let state = match self.state.as_mut() {
            Some(s) => s,
            None => {
                self.diag.imu_rejected_no_state += 1;
                return None;
            }
        };

        // Attitude staleness check
        if let Some(max_age) = self.config.max_attitude_age_ms
            && sample.time_ms.elapsed_ms(attitude_time) > max_age
        {
            self.diag.imu_rejected_stale_attitude += 1;
            return None;
        }

        // Check max duration
        if sample.time_ms.elapsed_ms(state.dr_start_ms) > self.config.max_dr_duration_ms {
            self.diag.imu_rejected_max_duration += 1;
            return None;
        }

        let dt_s = sample.time_ms.dt_seconds(state.time_ms);

        if dt_s < self.config.min_dt_s {
            self.diag.imu_rejected_min_dt += 1;
            return None;
        }
        if dt_s > self.config.max_dt_s {
            // Large gap: reset velocity, don't integrate
            self.diag.imu_rejected_gap += 1;
            state.vel_enu = Vector3::zeros();
            state.time_ms = sample.time_ms;
            return None;
        }

        // Acceleration in device frame (already calibrated)
        let a_device = Vector3::new(sample.accel[0], sample.accel[1], sample.accel[2]);

        // Rotate device → ENU using conjugate of Android quaternion
        let a_world = attitude.conjugate().transform_vector(&a_device);

        // Remove gravity (accelerometer reads +g on Z when stationary)
        let a_linear = Vector3::new(a_world.x, a_world.y, a_world.z - GRAVITY_MS2);

        // Semi-implicit Euler integration
        state.vel_enu += a_linear * dt_s;

        // ZUPT: zero velocity if magnitude below threshold
        if state.vel_enu.norm() < self.config.zupt_speed_threshold_mps {
            state.vel_enu = Vector3::zeros();
        }

        state.pos_enu += state.vel_enu * dt_s;
        state.time_ms = sample.time_ms;

        // Convert to lat/lon and emit
        let (lat, lon) = enu_to_latlon(state.pos_enu, state.anchor_lat_deg, state.anchor_lon_deg);

        let point = TrajectoryPoint {
            time_ms: sample.time_ms,
            latitude_deg: lat,
            longitude_deg: lon,
            altitude_m: state.anchor_alt_m + state.pos_enu.z,
            source: PointSource::DeadReckoning,
        };
        self.trajectory.push(point.clone());
        self.diag.imu_integrated += 1;
        Some(point)
    }

    /// Update device attitude from a quaternion sample.
    pub fn push_attitude(&mut self, attitude: &AttitudeSample) {
        self.diag.attitude_total += 1;
        self.attitude = Some((attitude.quaternion.to_unit_quaternion(), attitude.time_ms));
    }

    // ── Parser-coupled convenience API ──

    /// Dispatch a parsed record.
    ///
    /// Returns `Some(TrajectoryPoint)` when a trajectory point is emitted.
    /// Only Fix, UncalAccel, and GameRotationVector are used; others ignored.
    pub fn push_record(&mut self, record: &Record) -> Option<TrajectoryPoint> {
        match record {
            Record::Fix(f) => self.push_gnss(&GnssFix::from(f)),
            Record::UncalAccel(a) => self.push_imu(&ImuSample::from(a)),
            Record::GameRotationVector(g) => {
                self.push_attitude(&AttitudeSample::from(g));
                None
            }
            _ => None,
        }
    }

    /// Process all records from an iterator and return the full trajectory.
    ///
    /// Convenience method that feeds all records and calls [`finalize`](Self::finalize).
    pub fn process_all(
        mut self,
        records: impl IntoIterator<Item = Record>,
    ) -> Vec<TrajectoryPoint> {
        for record in records {
            self.push_record(&record);
        }
        self.finalize()
    }

    /// Return the trajectory.
    pub fn finalize(self) -> Vec<TrajectoryPoint> {
        self.trajectory
    }
}

// ────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────

fn meters_per_deg_lon(lat_deg: f64) -> f64 {
    METERS_PER_DEG_LAT * lat_deg.to_radians().cos()
}

fn enu_to_latlon(pos_enu: Vector3<f64>, anchor_lat: f64, anchor_lon: f64) -> (f64, f64) {
    let lat = anchor_lat + pos_enu.y / METERS_PER_DEG_LAT;
    let lon = anchor_lon + pos_enu.x / meters_per_deg_lon(anchor_lat);
    (lat, lon)
}

/// Initialize velocity from GNSS speed and bearing.
fn velocity_from_anchor(anchor: &GnssAnchor) -> Vector3<f64> {
    match (anchor.speed_mps, anchor.bearing_deg) {
        (Some(speed), Some(bearing)) if speed > 0.0 => {
            let rad = bearing.to_radians();
            Vector3::new(
                speed * rad.sin(), // East
                speed * rad.cos(), // North
                0.0,
            )
        }
        _ => Vector3::zeros(),
    }
}

// ────────────────────────────────────────────
// Post-processing smoothing
// ────────────────────────────────────────────

/// Convert lat/lon to ENU (east, north) relative to an anchor point.
fn latlon_to_enu(lat: f64, lon: f64, anchor_lat: f64, anchor_lon: f64) -> (f64, f64) {
    let east = (lon - anchor_lon) * meters_per_deg_lon(anchor_lat);
    let north = (lat - anchor_lat) * METERS_PER_DEG_LAT;
    (east, north)
}

/// Convert ENU (east, north) back to lat/lon relative to an anchor point.
fn enu_to_latlon_2d(east: f64, north: f64, anchor_lat: f64, anchor_lon: f64) -> (f64, f64) {
    let lat = anchor_lat + north / METERS_PER_DEG_LAT;
    let lon = anchor_lon + east / meters_per_deg_lon(anchor_lat);
    (lat, lon)
}

/// A DR segment bounded by GNSS fixes on both sides.
struct DrSegment {
    start_gnss: usize,
    end_gnss: usize,
    dr_start: usize,
    dr_end: usize, // inclusive
}

/// Find all DR segments bounded by GNSS points on both sides.
fn find_dr_segments(trajectory: &[TrajectoryPoint]) -> Vec<DrSegment> {
    let mut segments = Vec::new();
    let mut i = 0;
    while i < trajectory.len() {
        if trajectory[i].source == PointSource::DeadReckoning {
            // Find preceding GNSS point
            let start_gnss = if i > 0 {
                (0..i)
                    .rev()
                    .find(|&j| trajectory[j].source == PointSource::Gnss)
            } else {
                None
            };
            // Find end of DR run
            let dr_start = i;
            while i < trajectory.len() && trajectory[i].source == PointSource::DeadReckoning {
                i += 1;
            }
            let dr_end = i - 1;
            // Find following GNSS point
            let end_gnss =
                (i..trajectory.len()).find(|&j| trajectory[j].source == PointSource::Gnss);

            if let (Some(sg), Some(eg)) = (start_gnss, end_gnss) {
                segments.push(DrSegment {
                    start_gnss: sg,
                    end_gnss: eg,
                    dr_start,
                    dr_end,
                });
            }
        } else {
            i += 1;
        }
    }
    segments
}

/// Apply post-processing smoothing to a trajectory.
///
/// Identifies DR segments (contiguous `DeadReckoning` points bounded by `Gnss`
/// on both sides) and adjusts positions according to the chosen method.
/// GNSS points are never modified. Unbounded DR segments (no re-acquisition)
/// are left unchanged.
///
/// Smoothing operates in local ENU coordinates for geometric correctness,
/// and only adjusts horizontal position (altitude is preserved from DR).
pub fn smooth_trajectory(
    trajectory: &[TrajectoryPoint],
    method: SmoothingMethod,
) -> Vec<TrajectoryPoint> {
    let mut result: Vec<TrajectoryPoint> = trajectory.to_vec();
    let segments = find_dr_segments(&result);

    for seg in &segments {
        let start = &trajectory[seg.start_gnss];
        let end = &trajectory[seg.end_gnss];
        let t_start = start.time_ms.as_i64() as f64;
        let t_end = end.time_ms.as_i64() as f64;
        let dt = t_end - t_start;
        if dt <= 0.0 {
            continue;
        }

        let anchor_lat = start.latitude_deg;
        let anchor_lon = start.longitude_deg;
        let (end_east, end_north) =
            latlon_to_enu(end.latitude_deg, end.longitude_deg, anchor_lat, anchor_lon);

        match method {
            SmoothingMethod::Linear => {
                for pt in &mut result[seg.dr_start..=seg.dr_end] {
                    let t = pt.time_ms.as_i64() as f64;
                    let alpha = (t - t_start) / dt;
                    let east = alpha * end_east;
                    let north = alpha * end_north;
                    let (lat, lon) = enu_to_latlon_2d(east, north, anchor_lat, anchor_lon);
                    pt.latitude_deg = lat;
                    pt.longitude_deg = lon;
                    // altitude preserved
                }
            }
            SmoothingMethod::EndpointConstrained => {
                // Error = end_gnss_enu - last_dr_enu (horizontal only)
                let last_dr = &trajectory[seg.dr_end];
                let (last_dr_east, last_dr_north) = latlon_to_enu(
                    last_dr.latitude_deg,
                    last_dr.longitude_deg,
                    anchor_lat,
                    anchor_lon,
                );
                let error_east = end_east - last_dr_east;
                let error_north = end_north - last_dr_north;

                for i in seg.dr_start..=seg.dr_end {
                    let t = result[i].time_ms.as_i64() as f64;
                    let frac = (t - t_start) / dt;
                    // Quadratic weighting: correction ∝ t² (matches accel-bias error growth)
                    let alpha = frac * frac;

                    let (dr_east, dr_north) = latlon_to_enu(
                        trajectory[i].latitude_deg,
                        trajectory[i].longitude_deg,
                        anchor_lat,
                        anchor_lon,
                    );
                    let corrected_east = dr_east + alpha * error_east;
                    let corrected_north = dr_north + alpha * error_north;
                    let (lat, lon) =
                        enu_to_latlon_2d(corrected_east, corrected_north, anchor_lat, anchor_lon);
                    result[i].latitude_deg = lat;
                    result[i].longitude_deg = lon;
                    // altitude preserved
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FixProvider;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    /// Helper: create a UnitQuaternion from Android (x, y, z, w) convention.
    fn quat(x: f64, y: f64, z: f64, w: f64) -> UnitQuaternion<f64> {
        DeviceQuaternion { x, y, z, w }.to_unit_quaternion()
    }

    // ── TimestampMs ──

    #[test]
    fn timestamp_ms_dt_seconds() {
        let t1 = TimestampMs(1000);
        let t2 = TimestampMs(1100);
        assert!(approx_eq(t2.dt_seconds(t1), 0.1, 1e-10));
        assert!(approx_eq(
            TimestampMs(2000).dt_seconds(TimestampMs(1000)),
            1.0,
            1e-10
        ));
        // Negative dt (backwards time)
        assert!(approx_eq(t1.dt_seconds(t2), -0.1, 1e-10));
    }

    #[test]
    fn timestamp_ms_elapsed_ms() {
        let t1 = TimestampMs(1000);
        let t2 = TimestampMs(5000);
        assert_eq!(t2.elapsed_ms(t1), 4000);
        assert_eq!(t1.elapsed_ms(t2), -4000);
    }

    #[test]
    fn timestamp_ms_ordering() {
        let t1 = TimestampMs(100);
        let t2 = TimestampMs(200);
        assert!(t1 < t2);
        assert!(t2 > t1);
        assert_eq!(TimestampMs(100), TimestampMs(100));
    }

    #[test]
    fn timestamp_ms_display() {
        assert_eq!(format!("{}", TimestampMs(1234)), "1234ms");
        assert_eq!(format!("{}", TimestampMs(0)), "0ms");
    }

    #[test]
    fn timestamp_ms_as_i64() {
        assert_eq!(TimestampMs(42).as_i64(), 42);
    }

    // ── DeviceQuaternion ──

    #[test]
    fn device_quaternion_to_unit_identity() {
        // Android identity quaternion (x=0, y=0, z=0, w=1)
        let dq = DeviceQuaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        let uq = dq.to_unit_quaternion();
        // nalgebra identity: (w=1, i=0, j=0, k=0)
        assert!(approx_eq(uq.as_ref().w, 1.0, 1e-10));
        assert!(approx_eq(uq.as_ref().i, 0.0, 1e-10));
        assert!(approx_eq(uq.as_ref().j, 0.0, 1e-10));
        assert!(approx_eq(uq.as_ref().k, 0.0, 1e-10));
    }

    #[test]
    fn device_quaternion_to_unit_maps_xyzw_correctly() {
        // Verify Android (x,y,z,w) maps to nalgebra (w,i,j,k) = (w,x,y,z)
        let dq = DeviceQuaternion {
            x: 0.1,
            y: 0.2,
            z: 0.3,
            w: 0.9,
        };
        let uq = dq.to_unit_quaternion();
        let q = uq.quaternion();
        // nalgebra stores as (w, i, j, k) where i=x, j=y, k=z
        let norm = (0.1f64 * 0.1 + 0.2 * 0.2 + 0.3 * 0.3 + 0.9 * 0.9).sqrt();
        assert!(approx_eq(q.w, 0.9 / norm, 1e-10));
        assert!(approx_eq(q.i, 0.1 / norm, 1e-10));
        assert!(approx_eq(q.j, 0.2 / norm, 1e-10));
        assert!(approx_eq(q.k, 0.3 / norm, 1e-10));
    }

    #[test]
    fn device_quaternion_to_unit_normalizes() {
        // Non-unit quaternion should be normalized
        let dq = DeviceQuaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 2.0,
        };
        let uq = dq.to_unit_quaternion();
        let q = uq.quaternion();
        let norm = (q.w * q.w + q.i * q.i + q.j * q.j + q.k * q.k).sqrt();
        assert!(approx_eq(norm, 1.0, 1e-10));
    }

    // ── Quaternion convention ──

    #[test]
    fn quat_identity_preserves_vector() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        let result = UnitQuaternion::identity().transform_vector(&v);
        assert!(approx_eq(result.x, 1.0, 1e-10));
        assert!(approx_eq(result.y, 2.0, 1e-10));
        assert!(approx_eq(result.z, 3.0, 1e-10));
    }

    #[test]
    fn quat_90deg_around_z() {
        // 90° around Z: x=East(1,0,0) → y=North(0,1,0)
        let s = std::f64::consts::FRAC_PI_4.sin();
        let c = std::f64::consts::FRAC_PI_4.cos();
        let q = quat(0.0, 0.0, s, c);

        let result = q.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
        assert!(approx_eq(result.x, 0.0, 1e-10));
        assert!(approx_eq(result.y, 1.0, 1e-10));
        assert!(approx_eq(result.z, 0.0, 1e-10));
    }

    #[test]
    fn quat_conjugate_is_inverse() {
        let s = std::f64::consts::FRAC_PI_4.sin();
        let c = std::f64::consts::FRAC_PI_4.cos();
        let q = quat(0.0, 0.0, s, c);

        let v = Vector3::new(1.0, 2.0, 3.0);
        let back = q.conjugate().transform_vector(&q.transform_vector(&v));
        assert!(approx_eq(back.x, v.x, 1e-10));
        assert!(approx_eq(back.y, v.y, 1e-10));
        assert!(approx_eq(back.z, v.z, 1e-10));
    }

    #[test]
    fn device_to_enu_flat() {
        // Phone flat, screen up, top north → identity quaternion
        // Accel reads (0, 0, +g) → ENU should be (0, 0, +g)
        let q = UnitQuaternion::identity();
        let a_enu = q
            .conjugate()
            .transform_vector(&Vector3::new(0.0, 0.0, GRAVITY_MS2));
        assert!(approx_eq(a_enu.x, 0.0, 1e-10));
        assert!(approx_eq(a_enu.y, 0.0, 1e-10));
        assert!(approx_eq(a_enu.z, GRAVITY_MS2, 1e-10));
    }

    #[test]
    fn device_to_enu_upright() {
        // Phone upright, screen facing south, top up
        // ENU→device rotation: -90° around X
        let angle = -std::f64::consts::FRAC_PI_2;
        let q = quat((angle / 2.0).sin(), 0.0, 0.0, (angle / 2.0).cos());

        // Device accel: (0, +g, 0) (gravity along top edge = Up)
        let a_enu = q
            .conjugate()
            .transform_vector(&Vector3::new(0.0, GRAVITY_MS2, 0.0));
        assert!(approx_eq(a_enu.x, 0.0, 1e-10));
        assert!(approx_eq(a_enu.y, 0.0, 1e-10));
        assert!(approx_eq(a_enu.z, GRAVITY_MS2, 1e-10));
    }

    // ── Coordinate conversion ──

    #[test]
    fn enu_to_latlon_origin() {
        let (lat, lon) = enu_to_latlon(Vector3::zeros(), 36.0, 140.0);
        assert!(approx_eq(lat, 36.0, 1e-10));
        assert!(approx_eq(lon, 140.0, 1e-10));
    }

    #[test]
    fn enu_to_latlon_north() {
        // 111.132m north → +0.001° latitude
        let (lat, lon) = enu_to_latlon(Vector3::new(0.0, 111.132, 0.0), 36.0, 140.0);
        assert!(approx_eq(lat, 36.001, 1e-6));
        assert!(approx_eq(lon, 140.0, 1e-10));
    }

    // ── velocity_from_anchor ──

    #[test]
    fn velocity_east() {
        let anchor = GnssAnchor {
            lat_deg: 36.0,
            lon_deg: 140.0,
            alt_m: 100.0,
            speed_mps: Some(10.0),
            bearing_deg: Some(90.0),
        };
        let v = velocity_from_anchor(&anchor);
        assert!(approx_eq(v.x, 10.0, 1e-6));
        assert!(approx_eq(v.y, 0.0, 1e-6));
    }

    #[test]
    fn velocity_north() {
        let anchor = GnssAnchor {
            lat_deg: 36.0,
            lon_deg: 140.0,
            alt_m: 100.0,
            speed_mps: Some(5.0),
            bearing_deg: Some(0.0),
        };
        let v = velocity_from_anchor(&anchor);
        assert!(approx_eq(v.x, 0.0, 1e-6));
        assert!(approx_eq(v.y, 5.0, 1e-6));
    }

    #[test]
    fn velocity_no_speed() {
        let anchor = GnssAnchor {
            lat_deg: 36.0,
            lon_deg: 140.0,
            alt_m: 100.0,
            speed_mps: None,
            bearing_deg: None,
        };
        assert!(approx_eq(velocity_from_anchor(&anchor).norm(), 0.0, 1e-10));
    }

    // ── DeadReckoning integration ──

    fn make_fix(
        time_ms: i64,
        accuracy: f64,
        speed: Option<f64>,
        bearing: Option<f64>,
    ) -> FixRecord {
        FixRecord {
            provider: FixProvider::Gps,
            latitude_deg: 36.0,
            longitude_deg: 140.0,
            altitude_m: Some(100.0),
            speed_mps: speed,
            accuracy_m: Some(accuracy),
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

    fn make_accel(time_ms: i64, x: f64, y: f64, z: f64) -> UncalibratedSensorRecord {
        UncalibratedSensorRecord {
            utc_time_ms: time_ms,
            elapsed_realtime_ns: 0,
            x,
            y,
            z,
            bias_x: 0.0,
            bias_y: 0.0,
            bias_z: 0.0,
            calibration_accuracy: 3,
        }
    }

    fn make_grv(time_ms: i64, x: f64, y: f64, z: f64, w: f64) -> GameRotationVectorRecord {
        GameRotationVectorRecord {
            utc_time_ms: time_ms,
            elapsed_realtime_ns: 0,
            x,
            y,
            z,
            w,
        }
    }

    #[test]
    fn dr_stationary() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());

        // Good fix establishes anchor
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        // Degraded fix triggers DR
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        // Identity attitude (phone flat)
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));

        // Gravity-only accel for 1 second at 100Hz
        for i in 1..=100 {
            dr.push_imu(&ImuSample::from(&make_accel(
                2000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let traj = dr.finalize();
        assert_eq!(traj[0].source, PointSource::Gnss);

        // DR points should stay at anchor (stationary)
        for p in traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
        {
            assert!(
                approx_eq(p.latitude_deg, 36.0, 1e-6),
                "lat drift: {}",
                p.latitude_deg - 36.0
            );
            assert!(
                approx_eq(p.longitude_deg, 140.0, 1e-6),
                "lon drift: {}",
                p.longitude_deg - 140.0
            );
        }
    }

    #[test]
    fn dr_gnss_fusion() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());

        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None))); // DR starts
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        dr.push_imu(&ImuSample::from(&make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        dr.push_imu(&ImuSample::from(&make_accel(2020, 0.0, 0.0, GRAVITY_MS2)));
        dr.push_gnss(&GnssFix::from(&make_fix(3000, 5.0, Some(0.0), None))); // DR ends

        // Accel after good fix → no DR
        dr.push_imu(&ImuSample::from(&make_accel(3010, 0.0, 0.0, GRAVITY_MS2)));

        let traj = dr.finalize();
        let gnss: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::Gnss)
            .collect();
        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();

        assert_eq!(gnss.len(), 2);
        assert_eq!(dr_pts.len(), 2);
    }

    #[test]
    fn dr_max_duration() {
        let config = DeadReckoningConfig {
            max_dr_duration_ms: 100,
            ..DeadReckoningConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));

        dr.push_imu(&ImuSample::from(&make_accel(2050, 0.0, 0.0, GRAVITY_MS2))); // within
        dr.push_imu(&ImuSample::from(&make_accel(2200, 0.0, 0.0, GRAVITY_MS2))); // beyond

        let traj = dr.finalize();
        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();
        assert_eq!(dr_pts.len(), 1);
    }

    #[test]
    fn dr_no_anchor_no_dr() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());

        // Bad fix without prior good fix → no DR
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(1000, 0.0, 0.0, 0.0, 1.0)));
        dr.push_imu(&ImuSample::from(&make_accel(1010, 0.0, 0.0, GRAVITY_MS2)));

        assert!(dr.finalize().is_empty());
    }

    #[test]
    fn dr_constant_velocity_eastward() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0, // disable ZUPT
            max_attitude_age_ms: None,     // disable staleness (test is about velocity)
            ..DeadReckoningConfig::default()
        });

        // 10 m/s East
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(10.0), Some(90.0))));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));

        // 1 second of gravity-only (no linear accel), velocity should persist
        for i in 1..=100 {
            dr.push_imu(&ImuSample::from(&make_accel(
                2000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let traj = dr.finalize();
        let last_dr = traj
            .iter()
            .rev()
            .find(|p| p.source == PointSource::DeadReckoning)
            .unwrap();

        // ~10m east in 1 second
        let east_m = (last_dr.longitude_deg - 140.0) * meters_per_deg_lon(36.0);
        assert!(
            approx_eq(east_m, 10.0, 1.0),
            "expected ~10m east, got {east_m}m"
        );
        assert!(approx_eq(last_dr.latitude_deg, 36.0, 1e-5));
    }

    #[test]
    fn dr_zupt_prevents_drift() {
        let config = DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.5,
            ..DeadReckoningConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));

        for i in 1..=200 {
            dr.push_imu(&ImuSample::from(&make_accel(
                2000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let traj = dr.finalize();
        for p in traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
        {
            assert!(approx_eq(p.latitude_deg, 36.0, 1e-5));
            assert!(approx_eq(p.longitude_deg, 140.0, 1e-5));
        }
    }

    #[test]
    fn dr_large_gap_resets_velocity() {
        let config = DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            max_dt_s: 0.5,
            ..DeadReckoningConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(10.0), Some(90.0)))); // 10 m/s East
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));

        // Moving east
        for i in 1..=10 {
            dr.push_imu(&ImuSample::from(&make_accel(
                2000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // 2-second gap → velocity reset (no point emitted for this)
        dr.push_imu(&ImuSample::from(&make_accel(4100, 0.0, 0.0, GRAVITY_MS2)));

        // After reset: stationary
        for i in 1..=10 {
            dr.push_imu(&ImuSample::from(&make_accel(
                4100 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let traj = dr.finalize();
        let after: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning && p.time_ms > TimestampMs(4100))
            .collect();

        // Post-gap points should be nearly stationary relative to each other
        if after.len() >= 2 {
            let spread =
                (after.last().unwrap().longitude_deg - after.first().unwrap().longitude_deg).abs();
            assert!(
                spread < 1e-6,
                "should be stationary after gap, spread={spread}"
            );
        }
    }

    #[test]
    fn dr_push_dispatches() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());

        dr.push_record(&Record::Fix(make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_record(&Record::Fix(make_fix(2000, 50.0, None, None)));
        dr.push_record(&Record::GameRotationVector(make_grv(
            2000, 0.0, 0.0, 0.0, 1.0,
        )));
        dr.push_record(&Record::UncalAccel(make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        dr.push_record(&Record::Skipped); // ignored

        let traj = dr.finalize();
        assert_eq!(traj.len(), 2); // 1 GNSS + 1 DR
    }

    // ── Streaming output API ──

    #[test]
    fn push_gnss_returns_gnss_point() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        let fix = make_fix(1000, 5.0, Some(0.0), None);
        let result = dr.push_gnss(&GnssFix::from(&fix));
        assert!(result.is_some());
        let pt = result.unwrap();
        assert_eq!(pt.source, PointSource::Gnss);
        assert_eq!(pt.time_ms, TimestampMs(1000));
        assert!(approx_eq(pt.latitude_deg, 36.0, 1e-10));
    }

    #[test]
    fn push_gnss_degraded_returns_none() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        // Good fix first (anchor)
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        // Degraded fix starts DR but returns no point
        let result = dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        assert!(result.is_none());
    }

    #[test]
    fn push_imu_returns_dr_point() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        let result = dr.push_imu(&ImuSample::from(&make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        assert!(result.is_some());
        assert_eq!(result.unwrap().source, PointSource::DeadReckoning);
    }

    #[test]
    fn push_imu_no_state_returns_none() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        // No fix at all — accel should return None
        dr.push_attitude(&AttitudeSample::from(&make_grv(1000, 0.0, 0.0, 0.0, 1.0)));
        assert!(
            dr.push_imu(&ImuSample::from(&make_accel(1010, 0.0, 0.0, GRAVITY_MS2)))
                .is_none()
        );
    }

    #[test]
    fn push_record_returns_point_for_fix() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        let result = dr.push_record(&Record::Fix(make_fix(1000, 5.0, Some(0.0), None)));
        assert!(result.is_some());
        assert_eq!(result.unwrap().source, PointSource::Gnss);
    }

    #[test]
    fn push_record_returns_none_for_attitude() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        let result = dr.push_record(&Record::GameRotationVector(make_grv(
            1000, 0.0, 0.0, 0.0, 1.0,
        )));
        assert!(result.is_none());
    }

    #[test]
    fn push_record_returns_none_for_skipped() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        assert!(dr.push_record(&Record::Skipped).is_none());
    }

    #[test]
    fn process_all_matches_finalize() {
        let records = vec![
            Record::Fix(make_fix(1000, 5.0, Some(10.0), Some(90.0))),
            Record::Fix(make_fix(2000, 50.0, None, None)),
            Record::GameRotationVector(make_grv(2000, 0.0, 0.0, 0.0, 1.0)),
            Record::UncalAccel(make_accel(2010, 0.0, 0.0, GRAVITY_MS2)),
            Record::UncalAccel(make_accel(2020, 0.0, 0.0, GRAVITY_MS2)),
            Record::Fix(make_fix(3000, 5.0, Some(0.0), None)),
        ];

        // Manual push + finalize
        let mut dr1 = DeadReckoning::new(DeadReckoningConfig::default());
        for r in &records {
            dr1.push_record(r);
        }
        let traj1 = dr1.finalize();

        // process_all
        let dr2 = DeadReckoning::new(DeadReckoningConfig::default());
        let traj2 = dr2.process_all(records);

        assert_eq!(traj1.len(), traj2.len());
        for (a, b) in traj1.iter().zip(&traj2) {
            assert_eq!(a.time_ms, b.time_ms);
            assert_eq!(a.source, b.source);
        }
    }

    #[test]
    fn push_gnss_still_accumulates() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        let returned = dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        assert!(returned.is_some());
        // finalize should still contain the same point (backward compat)
        let traj = dr.finalize();
        assert_eq!(traj.len(), 1);
        assert_eq!(traj[0].time_ms, TimestampMs(1000));
        assert_eq!(traj[0].source, PointSource::Gnss);
    }

    // ── Attitude staleness ──

    #[test]
    fn attitude_staleness_rejects_stale() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        // IMU within staleness window → should produce DR point
        let result = dr.push_imu(&ImuSample::from(&make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        assert!(result.is_some());
        // IMU beyond staleness window (610ms after last attitude) → should return None
        let result = dr.push_imu(&ImuSample::from(&make_accel(2610, 0.0, 0.0, GRAVITY_MS2)));
        assert!(
            result.is_none(),
            "stale attitude should cause push_imu to return None"
        );
    }

    #[test]
    fn attitude_staleness_accepts_fresh() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        dr.push_imu(&ImuSample::from(&make_accel(2100, 0.0, 0.0, GRAVITY_MS2)));
        dr.push_imu(&ImuSample::from(&make_accel(2300, 0.0, 0.0, GRAVITY_MS2)));
        // Refresh attitude at t=2400 (attitude was getting stale at 400ms)
        dr.push_attitude(&AttitudeSample::from(&make_grv(2400, 0.0, 0.0, 0.0, 1.0)));
        // IMU at t=2500: attitude age 100ms (fresh), dt 200ms (ok)
        let result = dr.push_imu(&ImuSample::from(&make_accel(2500, 0.0, 0.0, GRAVITY_MS2)));
        assert!(
            result.is_some(),
            "fresh attitude should allow DR to continue"
        );
    }

    #[test]
    fn attitude_staleness_disabled() {
        let config = DeadReckoningConfig {
            max_attitude_age_ms: None,
            max_dt_s: 15.0,
            max_dr_duration_ms: 20_000,
            ..DeadReckoningConfig::default()
        };
        let mut dr = DeadReckoning::new(config);
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        // 10 seconds stale but check disabled (max_dt_s/max_dr_duration raised too)
        let result = dr.push_imu(&ImuSample::from(&make_accel(12000, 0.0, 0.0, GRAVITY_MS2)));
        assert!(
            result.is_some(),
            "staleness check disabled → should still emit DR"
        );
    }

    #[test]
    fn attitude_staleness_at_threshold() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        // Exactly at threshold (500ms) → accepted (> not >=)
        let result = dr.push_imu(&ImuSample::from(&make_accel(2500, 0.0, 0.0, GRAVITY_MS2)));
        assert!(result.is_some(), "exactly at threshold should be accepted");
    }

    #[test]
    fn attitude_staleness_custom_threshold() {
        let config = DeadReckoningConfig {
            max_attitude_age_ms: Some(200),
            ..DeadReckoningConfig::default()
        };
        let mut dr = DeadReckoning::new(config);
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        // 250ms > 200ms threshold
        let result = dr.push_imu(&ImuSample::from(&make_accel(2250, 0.0, 0.0, GRAVITY_MS2)));
        assert!(result.is_none(), "250ms > 200ms threshold → should reject");
    }

    #[test]
    fn attitude_staleness_resumes_after_refresh() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        dr.push_attitude(&AttitudeSample::from(&make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        dr.push_imu(&ImuSample::from(&make_accel(2100, 0.0, 0.0, GRAVITY_MS2)));
        dr.push_imu(&ImuSample::from(&make_accel(2300, 0.0, 0.0, GRAVITY_MS2)));
        // Stale: attitude age = 710ms > 500ms (staleness fires before dt check)
        let stale = dr.push_imu(&ImuSample::from(&make_accel(2710, 0.0, 0.0, GRAVITY_MS2)));
        assert!(stale.is_none(), "should reject stale");
        // Refresh attitude
        dr.push_attitude(&AttitudeSample::from(&make_grv(2800, 0.0, 0.0, 0.0, 1.0)));
        // First IMU: attitude fresh (10ms), but dt from last successful (2300) = 510ms
        // → gap handler resets velocity + updates time, returns None
        dr.push_imu(&ImuSample::from(&make_accel(2810, 0.0, 0.0, GRAVITY_MS2)));
        // Second IMU: attitude fresh (20ms), dt = 10ms → integration succeeds
        let fresh = dr.push_imu(&ImuSample::from(&make_accel(2820, 0.0, 0.0, GRAVITY_MS2)));
        assert!(fresh.is_some(), "should resume after attitude refresh");
    }

    #[test]
    fn attitude_staleness_no_attitude() {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        dr.push_gnss(&GnssFix::from(&make_fix(1000, 5.0, Some(0.0), None)));
        dr.push_gnss(&GnssFix::from(&make_fix(2000, 50.0, None, None)));
        // No attitude set, DR state active
        let result = dr.push_imu(&ImuSample::from(&make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        assert!(result.is_none(), "no attitude → push_imu returns None");
    }

    // ── Smoothing tests ──

    /// Build a simple trajectory: GNSS → DR × n → GNSS for smoothing tests.
    fn make_smoothing_trajectory(
        dr_offsets_m: &[(f64, f64)], // (east_m, north_m) relative to start GNSS
        end_gnss_offset_m: (f64, f64),
    ) -> Vec<TrajectoryPoint> {
        let base_lat = 36.0;
        let base_lon = 140.0;
        let mpdl = meters_per_deg_lon(base_lat);
        let n = dr_offsets_m.len();

        let mut traj = Vec::with_capacity(n + 2);
        // Start GNSS at t=0
        traj.push(TrajectoryPoint {
            time_ms: TimestampMs(0),
            latitude_deg: base_lat,
            longitude_deg: base_lon,
            altitude_m: 100.0,
            source: PointSource::Gnss,
        });
        // DR points at t=1000, 2000, ...
        for (i, &(east, north)) in dr_offsets_m.iter().enumerate() {
            traj.push(TrajectoryPoint {
                time_ms: TimestampMs((i as i64 + 1) * 1000),
                latitude_deg: base_lat + north / METERS_PER_DEG_LAT,
                longitude_deg: base_lon + east / mpdl,
                altitude_m: 100.0 + i as f64,
                source: PointSource::DeadReckoning,
            });
        }
        // End GNSS at t=(n+1)*1000
        traj.push(TrajectoryPoint {
            time_ms: TimestampMs((n as i64 + 1) * 1000),
            latitude_deg: base_lat + end_gnss_offset_m.1 / METERS_PER_DEG_LAT,
            longitude_deg: base_lon + end_gnss_offset_m.0 / mpdl,
            altitude_m: 110.0,
            source: PointSource::Gnss,
        });
        traj
    }

    #[test]
    fn smooth_linear_basic() {
        // DR drifts north, but end GNSS is east.
        // Linear should place DR points on a straight line east.
        let traj = make_smoothing_trajectory(
            &[
                (0.0, 10.0),
                (0.0, 20.0),
                (0.0, 30.0),
                (0.0, 40.0),
                (0.0, 50.0),
            ],
            (100.0, 0.0),
        );
        let smoothed = smooth_trajectory(&traj, SmoothingMethod::Linear);

        assert_eq!(smoothed.len(), traj.len());
        // DR points should be on a line from (0,0) to (100,0) in ENU
        let mpdl = meters_per_deg_lon(36.0);
        for (i, pt) in smoothed.iter().enumerate().take(6).skip(1) {
            let alpha = i as f64 / 6.0;
            let expected_east = alpha * 100.0;
            let actual_east = (pt.longitude_deg - 140.0) * mpdl;
            let actual_north = (pt.latitude_deg - 36.0) * METERS_PER_DEG_LAT;
            assert!(
                (actual_east - expected_east).abs() < 0.1,
                "point {i}: east={actual_east:.2}, expected={expected_east:.2}"
            );
            assert!(
                actual_north.abs() < 0.1,
                "point {i}: north={actual_north:.2}, expected=0"
            );
        }
    }

    #[test]
    fn smooth_endpoint_basic() {
        // DR drifts north, end GNSS is east.
        // EC should keep shape but pull last DR toward end GNSS.
        let traj = make_smoothing_trajectory(
            &[
                (0.0, 10.0),
                (0.0, 20.0),
                (0.0, 30.0),
                (0.0, 40.0),
                (0.0, 50.0),
            ],
            (100.0, 0.0),
        );
        let smoothed = smooth_trajectory(&traj, SmoothingMethod::EndpointConstrained);

        assert_eq!(smoothed.len(), traj.len());

        // First DR point (alpha=(1/6)²≈0.028) should be barely corrected
        let mpdl = meters_per_deg_lon(36.0);
        let first_north = (smoothed[1].latitude_deg - 36.0) * METERS_PER_DEG_LAT;
        assert!(
            (first_north - 10.0).abs() < 3.0,
            "first DR should barely move: north={first_north:.2}"
        );

        // Last DR point (alpha=(5/6)²≈0.694) should be significantly corrected
        let last_east = (smoothed[5].longitude_deg - 140.0) * mpdl;
        let last_north = (smoothed[5].latitude_deg - 36.0) * METERS_PER_DEG_LAT;
        // Should have moved substantially east (original was 0)
        assert!(
            last_east > 30.0,
            "last DR should move east: east={last_east:.2}"
        );
        // Should still have some northward component (shape preserved)
        assert!(
            last_north > 10.0,
            "last DR should retain some north: north={last_north:.2}"
        );
    }

    #[test]
    fn smooth_quadratic_weighting() {
        // Verify t² weighting: at alpha=0.5, correction should be 0.25 * error
        let traj = make_smoothing_trajectory(
            &[(50.0, 0.0), (100.0, 0.0)], // DR at 50m and 100m east
            (200.0, 0.0),                 // end GNSS at 200m east
        );
        let smoothed = smooth_trajectory(&traj, SmoothingMethod::EndpointConstrained);

        let mpdl = meters_per_deg_lon(36.0);
        // last_dr is at 100m east; end_gnss is at 200m east → error = 100m east
        // DR point at t=1000 (alpha = 1/3): correction = (1/3)² * 100 = 11.11m
        let p1_east = (smoothed[1].longitude_deg - 140.0) * mpdl;
        let expected_1 = 50.0 + (1.0 / 3.0_f64).powi(2) * 100.0;
        assert!(
            (p1_east - expected_1).abs() < 0.5,
            "t=1000: east={p1_east:.2}, expected={expected_1:.2}"
        );

        // DR point at t=2000 (alpha = 2/3): correction = (2/3)² * 100 = 44.44m
        let p2_east = (smoothed[2].longitude_deg - 140.0) * mpdl;
        let expected_2 = 100.0 + (2.0 / 3.0_f64).powi(2) * 100.0;
        assert!(
            (p2_east - expected_2).abs() < 0.5,
            "t=2000: east={p2_east:.2}, expected={expected_2:.2}"
        );
    }

    #[test]
    fn smooth_preserves_gnss() {
        let traj = make_smoothing_trajectory(&[(50.0, 50.0)], (100.0, 0.0));

        for method in [
            SmoothingMethod::Linear,
            SmoothingMethod::EndpointConstrained,
        ] {
            let smoothed = smooth_trajectory(&traj, method);
            // Start GNSS
            assert_eq!(smoothed[0].latitude_deg, traj[0].latitude_deg);
            assert_eq!(smoothed[0].longitude_deg, traj[0].longitude_deg);
            // End GNSS
            assert_eq!(smoothed[2].latitude_deg, traj[2].latitude_deg);
            assert_eq!(smoothed[2].longitude_deg, traj[2].longitude_deg);
        }
    }

    #[test]
    fn smooth_no_reacquire() {
        // DR at end with no following GNSS → should be unchanged
        let base_lat = 36.0;
        let base_lon = 140.0;
        let traj = vec![
            TrajectoryPoint {
                time_ms: TimestampMs(0),
                latitude_deg: base_lat,
                longitude_deg: base_lon,
                altitude_m: 100.0,
                source: PointSource::Gnss,
            },
            TrajectoryPoint {
                time_ms: TimestampMs(1000),
                latitude_deg: base_lat + 0.001,
                longitude_deg: base_lon,
                altitude_m: 100.0,
                source: PointSource::DeadReckoning,
            },
        ];
        for method in [
            SmoothingMethod::Linear,
            SmoothingMethod::EndpointConstrained,
        ] {
            let smoothed = smooth_trajectory(&traj, method);
            assert_eq!(smoothed[1].latitude_deg, traj[1].latitude_deg);
            assert_eq!(smoothed[1].longitude_deg, traj[1].longitude_deg);
        }
    }

    #[test]
    fn smooth_multiple_segments() {
        let base_lat = 36.0;
        let base_lon = 140.0;
        let mpdl = meters_per_deg_lon(base_lat);
        let traj = vec![
            TrajectoryPoint {
                time_ms: TimestampMs(0),
                latitude_deg: base_lat,
                longitude_deg: base_lon,
                altitude_m: 100.0,
                source: PointSource::Gnss,
            },
            TrajectoryPoint {
                time_ms: TimestampMs(1000),
                latitude_deg: base_lat + 50.0 / METERS_PER_DEG_LAT,
                longitude_deg: base_lon,
                altitude_m: 100.0,
                source: PointSource::DeadReckoning,
            },
            TrajectoryPoint {
                time_ms: TimestampMs(2000),
                latitude_deg: base_lat,
                longitude_deg: base_lon + 100.0 / mpdl,
                altitude_m: 100.0,
                source: PointSource::Gnss,
            },
            TrajectoryPoint {
                time_ms: TimestampMs(3000),
                latitude_deg: base_lat - 50.0 / METERS_PER_DEG_LAT,
                longitude_deg: base_lon + 100.0 / mpdl,
                altitude_m: 100.0,
                source: PointSource::DeadReckoning,
            },
            TrajectoryPoint {
                time_ms: TimestampMs(4000),
                latitude_deg: base_lat,
                longitude_deg: base_lon + 200.0 / mpdl,
                altitude_m: 100.0,
                source: PointSource::Gnss,
            },
        ];

        let smoothed = smooth_trajectory(&traj, SmoothingMethod::Linear);
        // Segment 1: DR[1] should be on line from GNSS[0] to GNSS[2]
        let p1_east = (smoothed[1].longitude_deg - base_lon) * mpdl;
        assert!(
            (p1_east - 50.0).abs() < 0.5,
            "seg1: east={p1_east:.2}, expected=50"
        );
        // Segment 2: DR[3] should be on line from GNSS[2] to GNSS[4]
        let p3_east = (smoothed[3].longitude_deg - base_lon) * mpdl;
        assert!(
            (p3_east - 150.0).abs() < 0.5,
            "seg2: east={p3_east:.2}, expected=150"
        );
    }

    #[test]
    fn smooth_altitude_unchanged() {
        let traj =
            make_smoothing_trajectory(&[(0.0, 10.0), (0.0, 20.0), (0.0, 30.0)], (100.0, 0.0));
        for method in [
            SmoothingMethod::Linear,
            SmoothingMethod::EndpointConstrained,
        ] {
            let smoothed = smooth_trajectory(&traj, method);
            for i in 1..=3 {
                assert_eq!(
                    smoothed[i].altitude_m, traj[i].altitude_m,
                    "altitude at {i} should be unchanged"
                );
            }
        }
    }

    #[test]
    fn smooth_empty() {
        let empty: Vec<TrajectoryPoint> = vec![];
        assert!(smooth_trajectory(&empty, SmoothingMethod::Linear).is_empty());
        assert!(smooth_trajectory(&empty, SmoothingMethod::EndpointConstrained).is_empty());
    }

    // ── SVG visualization helpers ──

    fn make_fix_at(
        time_ms: i64,
        accuracy: f64,
        speed: Option<f64>,
        bearing: Option<f64>,
        lat: f64,
        lon: f64,
    ) -> FixRecord {
        FixRecord {
            provider: FixProvider::Gps,
            latitude_deg: lat,
            longitude_deg: lon,
            altitude_m: Some(100.0),
            speed_mps: speed,
            accuracy_m: Some(accuracy),
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

    fn save_svg(name: &str, svg: &str) {
        let dir = format!("{}/assets/dr", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = format!("{dir}/{name}.svg");
        std::fs::write(&path, svg).unwrap();
        eprintln!("Wrote {path}");
    }

    // ── Comparison SVG renderer ──

    struct NamedTrajectory<'a> {
        label: &'a str,
        points: &'a [TrajectoryPoint],
        color: &'a str,
        dash: Option<&'a str>,
    }

    fn render_comparison_svg(
        trajectories: &[NamedTrajectory],
        title: &str,
        description: &str,
    ) -> String {
        let width = 800.0_f64;
        let height = 600.0_f64;
        let margin = 60.0;
        let plot_w = width - 2.0 * margin;
        let plot_h = height - 2.0 * margin;

        // Bounding box: GNSS points + smoothed trajectories (skip first = IMU-only).
        // IMU-only DR may drift far off; letting it clip makes the comparison clearer.
        let mut min_lon = f64::MAX;
        let mut max_lon = f64::MIN;
        let mut min_lat = f64::MAX;
        let mut max_lat = f64::MIN;
        // Always include GNSS points from the first trajectory
        if let Some(t) = trajectories.first() {
            for p in t.points.iter().filter(|p| p.source == PointSource::Gnss) {
                min_lon = min_lon.min(p.longitude_deg);
                max_lon = max_lon.max(p.longitude_deg);
                min_lat = min_lat.min(p.latitude_deg);
                max_lat = max_lat.max(p.latitude_deg);
            }
        }
        // Include all points from smoothed trajectories (index 1+)
        for t in trajectories.iter().skip(1) {
            for p in t.points {
                min_lon = min_lon.min(p.longitude_deg);
                max_lon = max_lon.max(p.longitude_deg);
                min_lat = min_lat.min(p.latitude_deg);
                max_lat = max_lat.max(p.latitude_deg);
            }
        }
        let pad_lon = (max_lon - min_lon).max(1e-7) * 0.1;
        let pad_lat = (max_lat - min_lat).max(1e-7) * 0.1;
        let lon_range = (max_lon - min_lon) + 2.0 * pad_lon;
        let lat_range = (max_lat - min_lat) + 2.0 * pad_lat;
        let bl = min_lon - pad_lon;
        let bb = min_lat - pad_lat;
        let to_x = |lon: f64| -> f64 { margin + (lon - bl) / lon_range * plot_w };
        let to_y = |lat: f64| -> f64 { margin + (1.0 - (lat - bb) / lat_range) * plot_h };

        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}">
<style>
  .title {{ font: bold 16px sans-serif; fill: #1f2937; }}
  .desc {{ font: 12px sans-serif; fill: #6b7280; }}
  .label {{ font: 10px monospace; fill: #374151; }}
  .grid {{ stroke: #e5e7eb; stroke-width: 0.5; }}
</style>
<rect width="{width}" height="{height}" fill="#fafafa" rx="4"/>
<text x="{margin}" y="24" class="title">{title}</text>
<text x="{margin}" y="42" class="desc">{description}</text>
"##
        );

        // Grid
        for i in 0..=4 {
            let y = margin + plot_h * i as f64 / 4.0;
            svg.push_str(&format!(
                r#"<line x1="{margin}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" class="grid"/>"#,
                margin + plot_w
            ));
            let x = margin + plot_w * i as f64 / 4.0;
            svg.push_str(&format!(
                r#"<line x1="{x:.1}" y1="{margin}" x2="{x:.1}" y2="{:.1}" class="grid"/>"#,
                margin + plot_h
            ));
        }

        // Collect GNSS points from first trajectory for shared markers
        let gnss_points: Vec<&TrajectoryPoint> = trajectories
            .first()
            .map(|t| {
                t.points
                    .iter()
                    .filter(|p| p.source == PointSource::Gnss)
                    .collect()
            })
            .unwrap_or_default();

        // GNSS path (shared)
        if !gnss_points.is_empty() {
            let mut d = String::new();
            for (i, p) in gnss_points.iter().enumerate() {
                let x = to_x(p.longitude_deg);
                let y = to_y(p.latitude_deg);
                let cmd = if i == 0 { "M" } else { "L" };
                d.push_str(&format!("{cmd}{x:.0},{y:.0} "));
            }
            svg.push_str(&format!(
                r##"<path d="{d}" fill="none" stroke="#2563eb" stroke-width="2" opacity="0.5"/>"##
            ));
        }

        // Each trajectory's DR path
        let max_dr_vis = 200;
        for t in trajectories {
            let dr_total = t
                .points
                .iter()
                .filter(|p| p.source == PointSource::DeadReckoning)
                .count();
            let step = (dr_total / max_dr_vis).max(1);
            let mut d = String::new();
            let mut dr_idx = 0usize;
            let mut first = true;
            for p in t.points {
                if p.source != PointSource::DeadReckoning {
                    continue;
                }
                if !dr_idx.is_multiple_of(step) {
                    dr_idx += 1;
                    continue;
                }
                dr_idx += 1;
                let x = to_x(p.longitude_deg);
                let y = to_y(p.latitude_deg);
                let cmd = if first { "M" } else { "L" };
                d.push_str(&format!("{cmd}{x:.0},{y:.0} "));
                first = false;
            }
            if !d.is_empty() {
                let dash = t
                    .dash
                    .map_or(String::new(), |d| format!(r#" stroke-dasharray="{d}""#));
                svg.push_str(&format!(
                    r#"<path d="{d}" fill="none" stroke="{}" stroke-width="2" opacity="0.7"{dash}/>"#,
                    t.color
                ));
            }
        }

        // GNSS point markers
        for p in &gnss_points {
            let x = to_x(p.longitude_deg);
            let y = to_y(p.latitude_deg);
            svg.push_str(&format!(
                r##"<circle cx="{x:.0}" cy="{y:.0}" r="4" fill="#2563eb" stroke="#2563eb"/>"##
            ));
        }

        // Legend
        let lx = width - 220.0;
        let mut ly = height - 70.0;
        svg.push_str(&format!(
            r##"<circle cx="{lx}" cy="{ly}" r="4" fill="#2563eb"/>
<text x="{:.1}" y="{:.1}" class="label">GNSS fix</text>"##,
            lx + 10.0,
            ly + 4.0,
        ));
        for t in trajectories {
            ly += 16.0;
            let dash = t
                .dash
                .map_or(String::new(), |d| format!(r#" stroke-dasharray="{d}""#));
            svg.push_str(&format!(
                r#"<line x1="{:.1}" y1="{ly}" x2="{:.1}" y2="{ly}" stroke="{}" stroke-width="2"{dash}/>
<text x="{:.1}" y="{:.1}" class="label">{}</text>"#,
                lx - 10.0,
                lx + 4.0,
                t.color,
                lx + 10.0,
                ly + 4.0,
                t.label,
            ));
        }

        svg.push_str("\n</svg>");
        svg
    }

    // ── Edge case visualization tests ──
    //
    // Each test builds the trajectory via a reusable builder, runs assertions,
    // then renders a smoothing-comparison SVG (IMU-only + Linear + EndpointConstrained).
    //
    // Run with: cargo test -p trajix dr_viz -- --ignored

    /// Helper: render comparison SVG for a trajectory and save it.
    fn save_comparison(trajectory: &[TrajectoryPoint], name: &str, title: &str, desc: &str) {
        let linear = smooth_trajectory(trajectory, SmoothingMethod::Linear);
        let ec = smooth_trajectory(trajectory, SmoothingMethod::EndpointConstrained);
        let svg = render_comparison_svg(
            &[
                NamedTrajectory {
                    label: "IMU-only",
                    points: trajectory,
                    color: "#dc2626",
                    dash: Some("6,3"),
                },
                NamedTrajectory {
                    label: "Linear",
                    points: &linear,
                    color: "#16a34a",
                    dash: Some("3,3"),
                },
                NamedTrajectory {
                    label: "EndpointConstrained",
                    points: &ec,
                    color: "#ea580c",
                    dash: Some("8,3,2,3"),
                },
            ],
            title,
            desc,
        );
        save_svg(name, &svg);
    }

    #[test]
    #[ignore]
    fn dr_viz_urban_canyon() {
        let traj = build_urban_canyon_trajectory();

        let gnss_count = traj
            .iter()
            .filter(|p| p.source == PointSource::Gnss)
            .count();
        let dr_count = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .count();
        assert!(gnss_count >= 20, "expected >=20 GNSS, got {gnss_count}");
        assert!(dr_count >= 40, "expected >=40 DR, got {dr_count}");

        let base_lat = 35.6812;
        let base_lon = 139.7001;
        for p in &traj {
            let dlat = (p.latitude_deg - base_lat).abs() * METERS_PER_DEG_LAT;
            let dlon = (p.longitude_deg - base_lon).abs() * meters_per_deg_lon(base_lat);
            let dist = (dlat * dlat + dlon * dlon).sqrt();
            assert!(dist < 200.0, "point drifted {dist:.0}m from start");
        }
        assert!(
            traj.last().unwrap().latitude_deg > traj.first().unwrap().latitude_deg,
            "should have northward trend"
        );

        save_comparison(
            &traj,
            "urban_canyon",
            "Urban Canyon",
            "Rapid GNSS good/bad alternation — 8 cycles, 3s good + 2s DR, walking north",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_tunnel_traverse() {
        let traj = build_tunnel_trajectory();

        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();
        assert!(
            dr_pts.len() > 100,
            "expected >100 DR points, got {}",
            dr_pts.len()
        );
        let first_dr = dr_pts.first().unwrap();
        let last_dr = dr_pts.last().unwrap();
        assert!(
            last_dr.longitude_deg > first_dr.longitude_deg,
            "DR should move east"
        );
        // Curved tunnel turns right — DR should move south
        assert!(
            last_dr.latitude_deg < first_dr.latitude_deg,
            "DR should curve south during 30° right turn"
        );

        save_comparison(
            &traj,
            "tunnel_traverse",
            "Tunnel Traverse (Curved)",
            "60s GNSS outage at 72 km/h — gentle 30° right curve inside tunnel",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_highway_turn() {
        let traj = build_highway_turn_trajectory();

        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();
        assert!(
            dr_pts.len() > 500,
            "expected >500 DR points, got {}",
            dr_pts.len()
        );
        let early: Vec<_> = dr_pts.iter().take(200).collect();
        assert!(
            early.last().unwrap().latitude_deg > early.first().unwrap().latitude_deg,
            "early DR should trend north"
        );

        save_comparison(
            &traj,
            "highway_turn",
            "Highway Turn",
            "30 m/s north → 90° turn east during 13s GNSS outage",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_stationary_noise() {
        let traj = build_stationary_noise_trajectory();

        let base_lat = 35.6812;
        let base_lon = 139.7671;
        for p in traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
        {
            let dlat = (p.latitude_deg - base_lat) * METERS_PER_DEG_LAT;
            let dlon = (p.longitude_deg - base_lon) * meters_per_deg_lon(base_lat);
            let dist = (dlat * dlat + dlon * dlon).sqrt();
            assert!(
                dist < 5.0,
                "stationary drift should be <5m, got {dist:.2}m at t={}ms",
                p.time_ms
            );
        }

        save_comparison(
            &traj,
            "stationary_noise",
            "Stationary with Noise",
            "30s GNSS outage while stationary — accelerometer noise, ZUPT active",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_max_duration_cutoff() {
        let traj = build_max_duration_trajectory();

        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();
        let dr_start = TimestampMs(1000);
        for p in &dr_pts {
            assert!(
                p.time_ms.elapsed_ms(dr_start) <= 30_000,
                "DR point at {} exceeds max duration (start={})",
                p.time_ms,
                dr_start
            );
        }
        if let Some(last_dr) = dr_pts.last() {
            assert!(last_dr.time_ms <= TimestampMs(31_000));
        }
        let exit_lon = 140.0 + 0.005;
        let last_dr_lon = dr_pts.last().unwrap().longitude_deg;
        assert!(
            exit_lon > last_dr_lon,
            "re-acquisition should be further east than last DR"
        );

        save_comparison(
            &traj,
            "max_duration_cutoff",
            "Max Duration Cutoff",
            "DR stops after 30s limit — gap visible before GNSS re-acquisition",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_s_curve() {
        let traj = build_s_curve_trajectory();
        let base_lat = 36.0;

        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == PointSource::DeadReckoning)
            .collect();
        assert!(
            dr_pts.len() > 500,
            "expected >500 DR points, got {}",
            dr_pts.len()
        );
        let lat_min = dr_pts
            .iter()
            .map(|p| p.latitude_deg)
            .fold(f64::MAX, f64::min);
        let lat_max = dr_pts
            .iter()
            .map(|p| p.latitude_deg)
            .fold(f64::MIN, f64::max);
        let lon_min = dr_pts
            .iter()
            .map(|p| p.longitude_deg)
            .fold(f64::MAX, f64::min);
        let lon_max = dr_pts
            .iter()
            .map(|p| p.longitude_deg)
            .fold(f64::MIN, f64::max);
        let lat_spread_m = (lat_max - lat_min) * METERS_PER_DEG_LAT;
        let lon_spread_m = (lon_max - lon_min) * meters_per_deg_lon(base_lat);
        assert!(
            lat_spread_m > 10.0,
            "should have latitude spread, got {lat_spread_m:.1}m"
        );
        assert!(
            lon_spread_m > 10.0,
            "should have longitude spread, got {lon_spread_m:.1}m"
        );

        save_comparison(
            &traj,
            "s_curve",
            "S-Curve",
            "20 m/s with north→east→south turns during 15s GNSS outage",
        );
    }

    // ── Reusable trajectory builders for smoothing comparison ──
    //
    // Run with: cargo test -p trajix dr_viz_compare -- --ignored

    /// Build an urban-canyon trajectory (reusable for comparison).
    fn build_urban_canyon_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            ..DeadReckoningConfig::default()
        });

        let base_lat = 35.6812;
        let base_lon = 139.7001;
        let speed = 1.4;
        let mut t: i64 = 0;

        for cycle in 0..8 {
            let cycle_start_lat = base_lat + (cycle as f64 * 5.0 * speed) / METERS_PER_DEG_LAT;
            for i in 0..3 {
                let lat = cycle_start_lat + (i as f64 * speed) / METERS_PER_DEG_LAT;
                dr.push_gnss(&GnssFix::from(&make_fix_at(
                    t,
                    3.0,
                    Some(speed),
                    Some(0.0),
                    lat,
                    base_lon,
                )));
                t += 1000;
            }
            dr.push_gnss(&GnssFix::from(&make_fix_at(
                t,
                100.0,
                None,
                None,
                cycle_start_lat,
                base_lon,
            )));
            t += 100;
            dr.push_attitude(&AttitudeSample::from(&make_grv(t, 0.0, 0.0, 0.0, 1.0)));
            for i in 0..200 {
                dr.push_imu(&ImuSample::from(&make_accel(
                    t + i * 10,
                    0.0,
                    0.2,
                    GRAVITY_MS2,
                )));
            }
            t += 2000;
        }
        dr.finalize()
    }

    /// Build a highway-turn trajectory (reusable for comparison).
    fn build_highway_turn_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            max_attitude_age_ms: None, // synthetic scenario — constant attitude phases
            ..DeadReckoningConfig::default()
        });

        let base_lat = 36.0;
        let base_lon = 140.0;
        let speed = 30.0;

        for i in 0..3 {
            let lat = base_lat + (i as f64 * speed) / METERS_PER_DEG_LAT;
            dr.push_gnss(&GnssFix::from(&make_fix_at(
                i * 1000,
                5.0,
                Some(speed),
                Some(0.0),
                lat,
                base_lon,
            )));
        }
        dr.push_gnss(&GnssFix::from(&make_fix_at(
            3000, 80.0, None, None, base_lat, base_lon,
        )));

        dr.push_attitude(&AttitudeSample::from(&make_grv(3000, 0.0, 0.0, 0.0, 1.0)));
        for i in 1..=500 {
            dr.push_imu(&ImuSample::from(&make_accel(
                3000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let turn_angle = std::f64::consts::FRAC_PI_2;
        let r = speed * 3.0 / turn_angle;
        let a_c = speed * speed / r;
        for i in 1..=300 {
            let frac = i as f64 / 300.0;
            let angle = frac * turn_angle;
            let half = angle / 2.0;
            dr.push_attitude(&AttitudeSample::from(&make_grv(
                8000 + i * 10,
                0.0,
                0.0,
                half.sin(),
                half.cos(),
            )));
            dr.push_imu(&ImuSample::from(&make_accel(
                8000 + i * 10,
                a_c,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let s90 = std::f64::consts::FRAC_PI_4.sin();
        let c90 = std::f64::consts::FRAC_PI_4.cos();
        dr.push_attitude(&AttitudeSample::from(&make_grv(11000, 0.0, 0.0, s90, c90)));
        for i in 1..=500 {
            dr.push_imu(&ImuSample::from(&make_accel(
                11000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            16000,
            5.0,
            Some(speed),
            Some(90.0),
            base_lat + 0.002,
            base_lon + 0.002,
        )));
        dr.finalize()
    }

    /// Build a stationary-noise trajectory (reusable for comparison).
    fn build_stationary_noise_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());

        let base_lat = 35.6812;
        let base_lon = 139.7671;

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            0,
            5.0,
            Some(0.0),
            None,
            base_lat,
            base_lon,
        )));
        dr.push_gnss(&GnssFix::from(&make_fix_at(
            1000, 50.0, None, None, base_lat, base_lon,
        )));
        dr.push_attitude(&AttitudeSample::from(&make_grv(1000, 0.0, 0.0, 0.0, 1.0)));

        for i in 1..=3000 {
            let t = 1000 + i * 10;
            let phase = i as f64 * 0.1;
            let nx = 0.05 * (phase * 1.7).sin() + 0.03 * (phase * 3.1).cos();
            let ny = 0.04 * (phase * 2.3).sin() + 0.02 * (phase * 4.7).cos();
            let nz = 0.02 * (phase * 0.9).sin();
            dr.push_imu(&ImuSample::from(&make_accel(t, nx, ny, GRAVITY_MS2 + nz)));
        }

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            31000,
            5.0,
            Some(0.0),
            None,
            base_lat,
            base_lon,
        )));
        dr.finalize()
    }

    /// Build a max-duration-cutoff trajectory (reusable for comparison).
    fn build_max_duration_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            max_dr_duration_ms: 30_000,
            ..DeadReckoningConfig::default()
        });

        let base_lat = 36.0;
        let base_lon = 140.0;

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            0,
            5.0,
            Some(5.0),
            Some(90.0),
            base_lat,
            base_lon,
        )));
        dr.push_gnss(&GnssFix::from(&make_fix_at(
            1000, 100.0, None, None, base_lat, base_lon,
        )));
        dr.push_attitude(&AttitudeSample::from(&make_grv(1000, 0.0, 0.0, 0.0, 1.0)));

        for i in 1..=6000 {
            dr.push_imu(&ImuSample::from(&make_accel(
                1000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        let exit_lon = base_lon + 0.005;
        dr.push_gnss(&GnssFix::from(&make_fix_at(
            62000,
            5.0,
            Some(5.0),
            Some(90.0),
            base_lat,
            exit_lon,
        )));
        dr.finalize()
    }

    /// Build a curved tunnel-traverse trajectory (reusable for comparison).
    ///
    /// Simulates a highway tunnel with a gentle 30° right curve:
    /// - Pre-tunnel: 5 GNSS fixes heading east at 72 km/h
    /// - Phase 1: 20s straight east (IMU coasting)
    /// - Phase 2: 10s gentle right curve (30°, radius ~382m)
    /// - Phase 3: 30s straight at bearing 120° (ESE)
    /// - Post-tunnel: 5 GNSS fixes at exit heading
    fn build_tunnel_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            max_dr_duration_ms: 120_000,
            max_attitude_age_ms: None, // synthetic — attitude pushed per phase
            ..DeadReckoningConfig::default()
        });

        let base_lat = 36.0;
        let base_lon = 140.0;
        let speed = 20.0; // m/s (~72 km/h)
        let mpdl = meters_per_deg_lon(base_lat);

        // Pre-tunnel: 5 GNSS fixes heading east
        for i in 0..5 {
            let lon = base_lon + (i as f64 * speed) / mpdl;
            dr.push_gnss(&GnssFix::from(&make_fix_at(
                i * 1000,
                5.0,
                Some(speed),
                Some(90.0),
                base_lat,
                lon,
            )));
        }

        // Tunnel entry — degraded GNSS
        dr.push_gnss(&GnssFix::from(&make_fix_at(
            5000, 200.0, None, None, base_lat, base_lon,
        )));

        // Heading angles (convention: 0=North, π/2=East, increases clockwise)
        let east_angle = std::f64::consts::FRAC_PI_2;
        let turn_rad = std::f64::consts::PI / 6.0; // 30°

        // Phase 1: 20s straight east
        let half_east = east_angle / 2.0;
        dr.push_attitude(&AttitudeSample::from(&make_grv(
            5000,
            0.0,
            0.0,
            half_east.sin(),
            half_east.cos(),
        )));
        for i in 1..=2000 {
            dr.push_imu(&ImuSample::from(&make_accel(
                5000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 2: 10s gentle right curve (30°)
        let turn_duration_s = 10.0;
        let turn_samples = 1000;
        let r = speed * turn_duration_s / turn_rad;
        let a_c = speed * speed / r;
        for i in 1..=turn_samples {
            let frac = i as f64 / turn_samples as f64;
            let angle = east_angle + frac * turn_rad;
            let half = angle / 2.0;
            dr.push_attitude(&AttitudeSample::from(&make_grv(
                25000 + i * 10,
                0.0,
                0.0,
                half.sin(),
                half.cos(),
            )));
            dr.push_imu(&ImuSample::from(&make_accel(
                25000 + i * 10,
                a_c,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 3: 30s straight at exit heading (bearing 120°)
        let exit_angle = east_angle + turn_rad;
        let half_exit = exit_angle / 2.0;
        dr.push_attitude(&AttitudeSample::from(&make_grv(
            35000,
            0.0,
            0.0,
            half_exit.sin(),
            half_exit.cos(),
        )));
        for i in 1..=3000 {
            dr.push_imu(&ImuSample::from(&make_accel(
                35000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Exit position (approximate from anchor at last good GNSS fix):
        //   Phase 1: 400m east, 0 north
        //   Phase 2: ~191m east, ~-51m north (30° curve)
        //   Phase 3: ~520m east, ~-300m north (bearing 120°)
        //   Total: ~1111m east, ~-351m south
        let exit_bearing_deg = 120.0_f64;
        let exit_bearing_rad = exit_bearing_deg.to_radians();
        let anchor_lon = base_lon + (4.0 * speed) / mpdl;
        let exit_lat = base_lat - 351.0 / METERS_PER_DEG_LAT;
        let exit_lon = anchor_lon + 1111.0 / mpdl;

        // Post-tunnel: 5 GNSS fixes heading ESE (bearing 120°)
        for i in 0..5 {
            let d = i as f64 * speed;
            let lat = exit_lat + d * exit_bearing_rad.cos() / METERS_PER_DEG_LAT;
            let lon = exit_lon + d * exit_bearing_rad.sin() / mpdl;
            dr.push_gnss(&GnssFix::from(&make_fix_at(
                65000 + i * 1000,
                5.0,
                Some(speed),
                Some(exit_bearing_deg),
                lat,
                lon,
            )));
        }
        dr.finalize()
    }

    /// Build an S-curve trajectory (reusable for comparison).
    fn build_s_curve_trajectory() -> Vec<TrajectoryPoint> {
        let mut dr = DeadReckoning::new(DeadReckoningConfig {
            zupt_speed_threshold_mps: 0.0,
            ..DeadReckoningConfig::default()
        });

        let base_lat = 36.0;
        let base_lon = 140.0;
        let speed = 20.0;

        for i in 0..3 {
            let lat = base_lat + (i as f64 * speed) / METERS_PER_DEG_LAT;
            dr.push_gnss(&GnssFix::from(&make_fix_at(
                i * 1000,
                5.0,
                Some(speed),
                Some(0.0),
                lat,
                base_lon,
            )));
        }

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            3000, 80.0, None, None, base_lat, base_lon,
        )));

        // Phase 1: 3s north
        dr.push_attitude(&AttitudeSample::from(&make_grv(3000, 0.0, 0.0, 0.0, 1.0)));
        for i in 1..=300 {
            dr.push_imu(&ImuSample::from(&make_accel(
                3000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 2: north→east turn
        let turn_angle = std::f64::consts::FRAC_PI_2;
        let r = speed * 3.0 / turn_angle;
        let a_c = speed * speed / r;
        for i in 1..=300 {
            let frac = i as f64 / 300.0;
            let angle = frac * turn_angle;
            let half = angle / 2.0;
            dr.push_attitude(&AttitudeSample::from(&make_grv(
                6000 + i * 10,
                0.0,
                0.0,
                half.sin(),
                half.cos(),
            )));
            dr.push_imu(&ImuSample::from(&make_accel(
                6000 + i * 10,
                a_c,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 3: 3s east
        let s90 = std::f64::consts::FRAC_PI_4.sin();
        let c90 = std::f64::consts::FRAC_PI_4.cos();
        dr.push_attitude(&AttitudeSample::from(&make_grv(9000, 0.0, 0.0, s90, c90)));
        for i in 1..=300 {
            dr.push_imu(&ImuSample::from(&make_accel(
                9000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 4: east→south turn
        for i in 1..=300 {
            let frac = i as f64 / 300.0;
            let angle = turn_angle + frac * turn_angle;
            let half = angle / 2.0;
            dr.push_attitude(&AttitudeSample::from(&make_grv(
                12000 + i * 10,
                0.0,
                0.0,
                half.sin(),
                half.cos(),
            )));
            dr.push_imu(&ImuSample::from(&make_accel(
                12000 + i * 10,
                a_c,
                0.0,
                GRAVITY_MS2,
            )));
        }

        // Phase 5: 3s south
        let s180 = std::f64::consts::FRAC_PI_2.sin();
        let c180 = std::f64::consts::FRAC_PI_2.cos();
        dr.push_attitude(&AttitudeSample::from(&make_grv(
            15000, 0.0, 0.0, s180, c180,
        )));
        for i in 1..=300 {
            dr.push_imu(&ImuSample::from(&make_accel(
                15000 + i * 10,
                0.0,
                0.0,
                GRAVITY_MS2,
            )));
        }

        dr.push_gnss(&GnssFix::from(&make_fix_at(
            18000,
            5.0,
            Some(speed),
            Some(180.0),
            base_lat,
            base_lon + 0.003,
        )));
        dr.finalize()
    }

    // ── Real data edge case extraction ──
    //
    // Parse actual GNSS log files, find DR segments, rank them by interest,
    // and generate comparison SVGs with smoothing methods.
    //
    // Run with: cargo test -p trajix dr_viz_real -- --ignored

    /// Parse a real GNSS log file through the DR pipeline.
    fn parse_real_log(filename: &str) -> Vec<TrajectoryPoint> {
        let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), filename);
        let file = std::fs::File::open(&path).unwrap_or_else(|e| {
            panic!("Could not open {path}: {e}. Real data file required for this test.");
        });
        let reader = std::io::BufReader::new(file);
        let parser = crate::parser::streaming::StreamingParser::new(reader);

        let mut dr = DeadReckoning::new(DeadReckoningConfig::default());
        for record in parser.flatten() {
            dr.push_record(&record);
        }
        dr.finalize()
    }

    /// Metadata about a discovered DR segment.
    #[allow(dead_code)]
    struct RealSegment {
        gnss_start: usize,
        gnss_end: usize,
        dr_start: usize,
        dr_end: usize,
        duration_ms: i64,
        dr_count: usize,
        endpoint_error_m: f64,
        avg_speed_mps: f64,
    }

    /// Find and rank all DR segments in a trajectory.
    fn find_real_segments(trajectory: &[TrajectoryPoint]) -> Vec<RealSegment> {
        let segments = find_dr_segments(trajectory);
        segments
            .iter()
            .map(|seg| {
                let start = &trajectory[seg.start_gnss];
                let end = &trajectory[seg.end_gnss];
                let last_dr = &trajectory[seg.dr_end];
                let duration_ms = end.time_ms.elapsed_ms(start.time_ms);
                let dr_count = seg.dr_end - seg.dr_start + 1;
                let endpoint_error_m = crate::geo::haversine_distance_m(
                    last_dr.latitude_deg,
                    last_dr.longitude_deg,
                    end.latitude_deg,
                    end.longitude_deg,
                );
                let dist_m = crate::geo::haversine_distance_m(
                    start.latitude_deg,
                    start.longitude_deg,
                    end.latitude_deg,
                    end.longitude_deg,
                );
                let dt_s = duration_ms as f64 / 1000.0;
                let avg_speed_mps = if dt_s > 0.0 { dist_m / dt_s } else { 0.0 };

                RealSegment {
                    gnss_start: seg.start_gnss,
                    gnss_end: seg.end_gnss,
                    dr_start: seg.dr_start,
                    dr_end: seg.dr_end,
                    duration_ms,
                    dr_count,
                    endpoint_error_m,
                    avg_speed_mps,
                }
            })
            .collect()
    }

    /// Extract a trajectory window around a segment (±context GNSS points).
    fn extract_window(
        trajectory: &[TrajectoryPoint],
        seg: &RealSegment,
        context: usize,
    ) -> Vec<TrajectoryPoint> {
        let mut before = Vec::new();
        for i in (0..seg.gnss_start).rev() {
            if trajectory[i].source == PointSource::Gnss {
                before.push(i);
                if before.len() >= context {
                    break;
                }
            }
        }
        before.reverse();

        let mut after = Vec::new();
        for (i, pt) in trajectory.iter().enumerate().skip(seg.gnss_end + 1) {
            if pt.source == PointSource::Gnss {
                after.push(i);
                if after.len() >= context {
                    break;
                }
            }
        }

        let start = before.first().copied().unwrap_or(seg.gnss_start);
        let end = after.last().copied().unwrap_or(seg.gnss_end);
        trajectory[start..=end].to_vec()
    }

    /// Extract a trajectory window spanning multiple consecutive segments.
    fn extract_multi_segment_window(
        trajectory: &[TrajectoryPoint],
        segments: &[&RealSegment],
        context: usize,
    ) -> Vec<TrajectoryPoint> {
        let first = segments.first().unwrap();
        let last = segments.last().unwrap();

        let mut before = Vec::new();
        for i in (0..first.gnss_start).rev() {
            if trajectory[i].source == PointSource::Gnss {
                before.push(i);
                if before.len() >= context {
                    break;
                }
            }
        }
        before.reverse();

        let mut after = Vec::new();
        for (i, pt) in trajectory.iter().enumerate().skip(last.gnss_end + 1) {
            if pt.source == PointSource::Gnss {
                after.push(i);
                if after.len() >= context {
                    break;
                }
            }
        }

        let start = before.first().copied().unwrap_or(first.gnss_start);
        let end = after.last().copied().unwrap_or(last.gnss_end);
        trajectory[start..=end].to_vec()
    }

    #[test]
    #[ignore]
    fn dr_viz_real_data_analysis() {
        for filename in [
            "gnss_log_test_8min.txt",
            "gnss_log_2025_11_29_10_31_31.txt",
            "gnss_log_2026_02_21_11_42_28.txt",
        ] {
            let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), filename);
            if !std::path::Path::new(&path).exists() {
                eprintln!("Skipping {filename} (not found)");
                continue;
            }
            eprintln!("\n=== Analyzing {filename} ===");
            let trajectory = parse_real_log(filename);

            let gnss_count = trajectory
                .iter()
                .filter(|p| p.source == PointSource::Gnss)
                .count();
            let dr_count = trajectory
                .iter()
                .filter(|p| p.source == PointSource::DeadReckoning)
                .count();
            eprintln!(
                "  Total: {} points (GNSS={gnss_count}, DR={dr_count})",
                trajectory.len()
            );

            let segments = find_real_segments(&trajectory);
            eprintln!("  DR segments: {}", segments.len());
            if segments.is_empty() {
                continue;
            }

            let mut by_duration: Vec<&RealSegment> = segments.iter().collect();
            by_duration.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));

            let mut by_error: Vec<&RealSegment> = segments.iter().collect();
            by_error.sort_by(|a, b| b.endpoint_error_m.partial_cmp(&a.endpoint_error_m).unwrap());

            let mut by_speed: Vec<&RealSegment> = segments.iter().collect();
            by_speed.sort_by(|a, b| b.avg_speed_mps.partial_cmp(&a.avg_speed_mps).unwrap());

            eprintln!("\n  Top 5 by duration:");
            for s in by_duration.iter().take(5) {
                eprintln!(
                    "    {:.1}s  DR={:>4}pts  err={:.1}m  speed={:.1}m/s",
                    s.duration_ms as f64 / 1000.0,
                    s.dr_count,
                    s.endpoint_error_m,
                    s.avg_speed_mps,
                );
            }

            eprintln!("\n  Top 5 by endpoint error:");
            for s in by_error.iter().take(5) {
                eprintln!(
                    "    err={:.1}m  dur={:.1}s  DR={:>4}pts  speed={:.1}m/s",
                    s.endpoint_error_m,
                    s.duration_ms as f64 / 1000.0,
                    s.dr_count,
                    s.avg_speed_mps,
                );
            }

            eprintln!("\n  Top 5 by speed:");
            for s in by_speed.iter().take(5) {
                eprintln!(
                    "    speed={:.1}m/s ({:.0}km/h)  dur={:.1}s  DR={:>4}pts  err={:.1}m",
                    s.avg_speed_mps,
                    s.avg_speed_mps * 3.6,
                    s.duration_ms as f64 / 1000.0,
                    s.dr_count,
                    s.endpoint_error_m,
                );
            }

            let dur_buckets = [
                (0.0, 5.0, "<5s"),
                (5.0, 15.0, "5-15s"),
                (15.0, 30.0, "15-30s"),
                (30.0, 60.0, "30-60s"),
                (60.0, f64::MAX, "60s+"),
            ];
            eprint!("\n  Duration distribution: ");
            for (lo, hi, label) in &dur_buckets {
                let count = segments
                    .iter()
                    .filter(|s| {
                        let d = s.duration_ms as f64 / 1000.0;
                        d >= *lo && d < *hi
                    })
                    .count();
                if count > 0 {
                    eprint!("{label}={count} ");
                }
            }
            eprintln!();
        }
    }

    /// Generate comparison SVGs for the most interesting segments in a log file.
    fn generate_real_data_svgs(filename: &str, prefix: &str, label: &str) {
        let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), filename);
        if !std::path::Path::new(&path).exists() {
            panic!("{filename} not found — required for this test");
        }
        eprintln!("Parsing {filename}...");
        let trajectory = parse_real_log(filename);
        let segments = find_real_segments(&trajectory);
        eprintln!(
            "  {} points, {} DR segments",
            trajectory.len(),
            segments.len()
        );

        if segments.is_empty() {
            eprintln!("  No DR segments found.");
            return;
        }

        // 1. Longest segment
        let mut by_duration: Vec<&RealSegment> = segments.iter().collect();
        by_duration.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
        if let Some(seg) = by_duration.first() {
            let window = extract_window(&trajectory, seg, 5);
            save_comparison(
                &window,
                &format!("real_{prefix}_longest"),
                &format!("{label} — Longest DR gap"),
                &format!(
                    "Real data: {:.1}s gap, {} DR points, endpoint error {:.1}m",
                    seg.duration_ms as f64 / 1000.0,
                    seg.dr_count,
                    seg.endpoint_error_m,
                ),
            );
        }

        // 2. Worst endpoint error (skip if same as longest)
        let mut by_error: Vec<&RealSegment> = segments.iter().collect();
        by_error.sort_by(|a, b| b.endpoint_error_m.partial_cmp(&a.endpoint_error_m).unwrap());
        if let Some(seg) = by_error
            .iter()
            .find(|s| s.dr_start != by_duration[0].dr_start)
        {
            let window = extract_window(&trajectory, seg, 5);
            save_comparison(
                &window,
                &format!("real_{prefix}_worst_drift"),
                &format!("{label} — Worst DR drift"),
                &format!(
                    "Real data: endpoint error {:.1}m, {:.1}s gap, {:.1}m/s",
                    seg.endpoint_error_m,
                    seg.duration_ms as f64 / 1000.0,
                    seg.avg_speed_mps,
                ),
            );
        }

        // 3. Highest speed segment
        let mut by_speed: Vec<&RealSegment> = segments.iter().collect();
        by_speed.sort_by(|a, b| b.avg_speed_mps.partial_cmp(&a.avg_speed_mps).unwrap());
        if let Some(seg) = by_speed.first()
            && seg.dr_start != by_duration[0].dr_start
            && seg.dr_start != by_error[0].dr_start
            && seg.avg_speed_mps > 1.0
        {
            let window = extract_window(&trajectory, seg, 5);
            save_comparison(
                &window,
                &format!("real_{prefix}_high_speed"),
                &format!("{label} — High-speed DR"),
                &format!(
                    "Real data: {:.1}m/s ({:.0}km/h), {:.1}s gap, err={:.1}m",
                    seg.avg_speed_mps,
                    seg.avg_speed_mps * 3.6,
                    seg.duration_ms as f64 / 1000.0,
                    seg.endpoint_error_m,
                ),
            );
        }

        // 4. Find cluster of frequent gaps (urban-canyon-like)
        // Look for windows where ≥3 segments occur within 60s
        let mut best_cluster: Option<Vec<&RealSegment>> = None;
        let mut best_cluster_count = 0;
        for i in 0..segments.len() {
            let t_start =
                segments[i].duration_ms + trajectory[segments[i].gnss_start].time_ms.as_i64();
            let cluster: Vec<&RealSegment> = segments[i..]
                .iter()
                .take_while(|s| {
                    trajectory[s.gnss_end]
                        .time_ms
                        .elapsed_ms(trajectory[segments[i].gnss_start].time_ms)
                        < 60_000
                })
                .collect();
            if cluster.len() >= 3 && cluster.len() > best_cluster_count {
                let _ = t_start;
                best_cluster_count = cluster.len();
                best_cluster = Some(cluster);
            }
        }
        if let Some(cluster) = best_cluster {
            let window = extract_multi_segment_window(&trajectory, &cluster, 3);
            let total_dr: usize = cluster.iter().map(|s| s.dr_count).sum();
            save_comparison(
                &window,
                &format!("real_{prefix}_frequent_gaps"),
                &format!("{label} — Frequent gaps"),
                &format!(
                    "Real data: {} gaps in {:.0}s window, {} total DR points",
                    cluster.len(),
                    trajectory[cluster.last().unwrap().gnss_end]
                        .time_ms
                        .elapsed_ms(trajectory[cluster.first().unwrap().gnss_start].time_ms)
                        as f64
                        / 1000.0,
                    total_dr,
                ),
            );
        }

        // 5. Shortest non-trivial segment (≥3 DR points)
        let mut short: Vec<&RealSegment> = segments.iter().filter(|s| s.dr_count >= 3).collect();
        short.sort_by(|a, b| a.duration_ms.cmp(&b.duration_ms));
        if let Some(seg) = short.first()
            && seg.dr_start != by_duration[0].dr_start
        {
            let window = extract_window(&trajectory, seg, 5);
            save_comparison(
                &window,
                &format!("real_{prefix}_shortest"),
                &format!("{label} — Shortest DR gap"),
                &format!(
                    "Real data: {:.1}s gap, {} DR points, err={:.1}m",
                    seg.duration_ms as f64 / 1000.0,
                    seg.dr_count,
                    seg.endpoint_error_m,
                ),
            );
        }
    }

    #[test]
    #[ignore]
    fn dr_viz_real_flight() {
        generate_real_data_svgs(
            "gnss_log_2025_11_29_10_31_31.txt",
            "flight",
            "Chitose→Narita Flight",
        );
    }

    #[test]
    #[ignore]
    fn dr_viz_real_hiking() {
        generate_real_data_svgs(
            "gnss_log_2026_02_21_11_42_28.txt",
            "hiking",
            "Mt. Tsukuba Hiking",
        );
    }
}
