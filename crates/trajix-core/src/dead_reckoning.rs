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
//! 4. Integrate acceleration → velocity → position (Euler method)
//! 5. Convert local ENU position to lat/lon using the GNSS anchor point
//!
//! DR activates when GNSS accuracy exceeds a configurable threshold and
//! deactivates when a good GNSS fix arrives.

use crate::parser::line::Record;
use crate::record::fix::FixRecord;
use crate::record::sensor::{GameRotationVectorRecord, UncalibratedSensorRecord};

/// Standard gravity (m/s²).
const GRAVITY_MS2: f64 = 9.80665;

/// Meters per degree of latitude (approximate, WGS84 mean).
const METERS_PER_DEG_LAT: f64 = 111_132.0;

// ────────────────────────────────────────────
// Vec3
// ────────────────────────────────────────────

/// 3D vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn zero() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    pub fn magnitude(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

// ────────────────────────────────────────────
// Quaternion
// ────────────────────────────────────────────

/// Unit quaternion (Hamilton convention: q = w + xi + yj + zk).
///
/// Follows Android `GameRotationVector` convention:
/// the quaternion rotates ENU (world) → device frame.
/// Use `q.conjugate().rotate(v_device)` to get `v_enu`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quat {
    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    pub fn identity() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }

    /// Conjugate (= inverse for unit quaternions).
    pub fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Rotate vector v: v' = q v q*.
    ///
    /// Optimized formula: v' = v + 2w(u × v) + 2(u × (u × v))
    /// where u = (x, y, z) is the vector part of q.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = cross(u, v) * 2.0;
        v + t * self.w + cross(u, t)
    }
}

// ────────────────────────────────────────────
// Dead Reckoning types
// ────────────────────────────────────────────

/// Configuration for the Dead Reckoning processor.
#[derive(Debug, Clone)]
pub struct DrConfig {
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
}

impl Default for DrConfig {
    fn default() -> Self {
        Self {
            accuracy_threshold_m: 30.0,
            zupt_speed_threshold_mps: 0.3,
            max_dr_duration_ms: 120_000,
            min_dt_s: 0.001,
            max_dt_s: 0.5,
        }
    }
}

/// Source of a trajectory point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrSource {
    Gnss,
    DeadReckoning,
}

/// A single trajectory point.
#[derive(Debug, Clone)]
pub struct DrPoint {
    pub time_ms: i64,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub source: DrSource,
}

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
    time_ms: i64,
    pos_enu: Vec3,
    vel_enu: Vec3,
    anchor_lat_deg: f64,
    anchor_lon_deg: f64,
    anchor_alt_m: f64,
    dr_start_ms: i64,
}

// ────────────────────────────────────────────
// DeadReckoning processor
// ────────────────────────────────────────────

/// Streaming Dead Reckoning processor.
///
/// Feed records in chronological order via [`push`] or the type-specific
/// methods, then call [`finalize`] to get the merged trajectory.
pub struct DeadReckoning {
    config: DrConfig,
    last_fix: Option<GnssAnchor>,
    state: Option<DrState>,
    attitude: Option<Quat>,
    trajectory: Vec<DrPoint>,
}

impl DeadReckoning {
    pub fn new(config: DrConfig) -> Self {
        Self {
            config,
            last_fix: None,
            state: None,
            attitude: None,
            trajectory: Vec::new(),
        }
    }

    /// Dispatch a parsed record.
    ///
    /// Only Fix, UncalAccel, and GameRotationVector are used; others ignored.
    pub fn push(&mut self, record: &Record) {
        match record {
            Record::Fix(f) => self.push_fix(f),
            Record::UncalAccel(a) => self.push_accel(a),
            Record::GameRotationVector(g) => self.push_attitude(g),
            _ => {}
        }
    }

    /// Process a GNSS fix.
    pub fn push_fix(&mut self, fix: &FixRecord) {
        let accuracy = fix.accuracy_m.unwrap_or(f64::MAX);

        if accuracy <= self.config.accuracy_threshold_m {
            // Good fix: end DR, update anchor, emit point
            self.state = None;
            self.last_fix = Some(GnssAnchor {
                lat_deg: fix.latitude_deg,
                lon_deg: fix.longitude_deg,
                alt_m: fix.altitude_m.unwrap_or(0.0),
                speed_mps: fix.speed_mps,
                bearing_deg: fix.bearing_deg,
            });
            self.trajectory.push(DrPoint {
                time_ms: fix.unix_time_ms,
                latitude_deg: fix.latitude_deg,
                longitude_deg: fix.longitude_deg,
                altitude_m: fix.altitude_m.unwrap_or(0.0),
                source: DrSource::Gnss,
            });
        } else if self.state.is_none() {
            // Degraded fix: start DR from last good anchor
            if let Some(anchor) = &self.last_fix {
                let vel = velocity_from_anchor(anchor);
                self.state = Some(DrState {
                    time_ms: fix.unix_time_ms,
                    pos_enu: Vec3::zero(),
                    vel_enu: vel,
                    anchor_lat_deg: anchor.lat_deg,
                    anchor_lon_deg: anchor.lon_deg,
                    anchor_alt_m: anchor.alt_m,
                    dr_start_ms: fix.unix_time_ms,
                });
            }
        }
    }

    /// Update attitude from a GameRotationVector reading.
    pub fn push_attitude(&mut self, grv: &GameRotationVectorRecord) {
        self.attitude = Some(Quat::new(grv.x, grv.y, grv.z, grv.w));
    }

    /// Process an uncalibrated accelerometer reading.
    pub fn push_accel(&mut self, accel: &UncalibratedSensorRecord) {
        let attitude = match self.attitude {
            Some(q) => q,
            None => return,
        };

        let state = match &mut self.state {
            Some(s) => s,
            None => return,
        };

        // Check max duration
        if accel.utc_time_ms - state.dr_start_ms > self.config.max_dr_duration_ms {
            return;
        }

        let dt_s = (accel.utc_time_ms - state.time_ms) as f64 / 1000.0;

        if dt_s < self.config.min_dt_s {
            return;
        }
        if dt_s > self.config.max_dt_s {
            // Large gap: reset velocity, don't integrate
            state.vel_enu = Vec3::zero();
            state.time_ms = accel.utc_time_ms;
            return;
        }

        // Calibrated acceleration in device frame
        let a_device = Vec3::new(
            accel.x - accel.bias_x,
            accel.y - accel.bias_y,
            accel.z - accel.bias_z,
        );

        // Rotate device → ENU using conjugate of Android quaternion
        let a_world = attitude.conjugate().rotate(a_device);

        // Remove gravity (accelerometer reads +g on Z when stationary)
        let a_linear = Vec3::new(a_world.x, a_world.y, a_world.z - GRAVITY_MS2);

        // Euler integration
        state.vel_enu = state.vel_enu + a_linear * dt_s;

        // ZUPT: zero velocity if magnitude below threshold
        if state.vel_enu.magnitude() < self.config.zupt_speed_threshold_mps {
            state.vel_enu = Vec3::zero();
        }

        state.pos_enu = state.pos_enu + state.vel_enu * dt_s;
        state.time_ms = accel.utc_time_ms;

        // Convert to lat/lon and emit
        let (lat, lon) = enu_to_latlon(state.pos_enu, state.anchor_lat_deg, state.anchor_lon_deg);

        self.trajectory.push(DrPoint {
            time_ms: accel.utc_time_ms,
            latitude_deg: lat,
            longitude_deg: lon,
            altitude_m: state.anchor_alt_m + state.pos_enu.z,
            source: DrSource::DeadReckoning,
        });
    }

    /// Return the trajectory.
    pub fn finalize(self) -> Vec<DrPoint> {
        self.trajectory
    }
}

// ────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────

fn meters_per_deg_lon(lat_deg: f64) -> f64 {
    METERS_PER_DEG_LAT * lat_deg.to_radians().cos()
}

fn enu_to_latlon(pos_enu: Vec3, anchor_lat: f64, anchor_lon: f64) -> (f64, f64) {
    let lat = anchor_lat + pos_enu.y / METERS_PER_DEG_LAT;
    let lon = anchor_lon + pos_enu.x / meters_per_deg_lon(anchor_lat);
    (lat, lon)
}

/// Initialize velocity from GNSS speed and bearing.
fn velocity_from_anchor(anchor: &GnssAnchor) -> Vec3 {
    match (anchor.speed_mps, anchor.bearing_deg) {
        (Some(speed), Some(bearing)) if speed > 0.0 => {
            let rad = bearing.to_radians();
            Vec3::new(
                speed * rad.sin(), // East
                speed * rad.cos(), // North
                0.0,
            )
        }
        _ => Vec3::zero(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FixProvider;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // ── Vec3 ──

    #[test]
    fn vec3_magnitude() {
        assert!(approx_eq(Vec3::new(3.0, 4.0, 0.0).magnitude(), 5.0, 1e-10));
        assert!(approx_eq(Vec3::zero().magnitude(), 0.0, 1e-10));
    }

    #[test]
    fn vec3_ops() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);

        let sum = a + b;
        assert!(approx_eq(sum.x, 5.0, 1e-10));
        assert!(approx_eq(sum.y, 7.0, 1e-10));
        assert!(approx_eq(sum.z, 9.0, 1e-10));

        let diff = a - b;
        assert!(approx_eq(diff.x, -3.0, 1e-10));

        let scaled = a * 2.0;
        assert!(approx_eq(scaled.x, 2.0, 1e-10));
        assert!(approx_eq(scaled.y, 4.0, 1e-10));
    }

    // ── Quaternion ──

    #[test]
    fn quat_identity_preserves_vector() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        let result = Quat::identity().rotate(v);
        assert!(approx_eq(result.x, 1.0, 1e-10));
        assert!(approx_eq(result.y, 2.0, 1e-10));
        assert!(approx_eq(result.z, 3.0, 1e-10));
    }

    #[test]
    fn quat_90deg_around_z() {
        // 90° around Z: x=East(1,0,0) → y=North(0,1,0)
        let s = std::f64::consts::FRAC_PI_4.sin();
        let c = std::f64::consts::FRAC_PI_4.cos();
        let q = Quat::new(0.0, 0.0, s, c);

        let result = q.rotate(Vec3::new(1.0, 0.0, 0.0));
        assert!(approx_eq(result.x, 0.0, 1e-10));
        assert!(approx_eq(result.y, 1.0, 1e-10));
        assert!(approx_eq(result.z, 0.0, 1e-10));
    }

    #[test]
    fn quat_conjugate_is_inverse() {
        let s = std::f64::consts::FRAC_PI_4.sin();
        let c = std::f64::consts::FRAC_PI_4.cos();
        let q = Quat::new(0.0, 0.0, s, c);

        let v = Vec3::new(1.0, 2.0, 3.0);
        let back = q.conjugate().rotate(q.rotate(v));
        assert!(approx_eq(back.x, v.x, 1e-10));
        assert!(approx_eq(back.y, v.y, 1e-10));
        assert!(approx_eq(back.z, v.z, 1e-10));
    }

    #[test]
    fn device_to_enu_flat() {
        // Phone flat, screen up, top north → identity quaternion
        // Accel reads (0, 0, +g) → ENU should be (0, 0, +g)
        let q = Quat::identity();
        let a_enu = q.conjugate().rotate(Vec3::new(0.0, 0.0, GRAVITY_MS2));
        assert!(approx_eq(a_enu.x, 0.0, 1e-10));
        assert!(approx_eq(a_enu.y, 0.0, 1e-10));
        assert!(approx_eq(a_enu.z, GRAVITY_MS2, 1e-10));
    }

    #[test]
    fn device_to_enu_upright() {
        // Phone upright, screen facing south, top up
        // ENU→device rotation: -90° around X
        let angle = -std::f64::consts::FRAC_PI_2;
        let q = Quat::new((angle / 2.0).sin(), 0.0, 0.0, (angle / 2.0).cos());

        // Device accel: (0, +g, 0) (gravity along top edge = Up)
        let a_enu = q.conjugate().rotate(Vec3::new(0.0, GRAVITY_MS2, 0.0));
        assert!(approx_eq(a_enu.x, 0.0, 1e-10));
        assert!(approx_eq(a_enu.y, 0.0, 1e-10));
        assert!(approx_eq(a_enu.z, GRAVITY_MS2, 1e-10));
    }

    // ── Coordinate conversion ──

    #[test]
    fn enu_to_latlon_origin() {
        let (lat, lon) = enu_to_latlon(Vec3::zero(), 36.0, 140.0);
        assert!(approx_eq(lat, 36.0, 1e-10));
        assert!(approx_eq(lon, 140.0, 1e-10));
    }

    #[test]
    fn enu_to_latlon_north() {
        // 111.132m north → +0.001° latitude
        let (lat, lon) = enu_to_latlon(Vec3::new(0.0, 111.132, 0.0), 36.0, 140.0);
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
        assert!(approx_eq(velocity_from_anchor(&anchor).magnitude(), 0.0, 1e-10));
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
        let mut dr = DeadReckoning::new(DrConfig::default());

        // Good fix establishes anchor
        dr.push_fix(&make_fix(1000, 5.0, Some(0.0), None));
        // Degraded fix triggers DR
        dr.push_fix(&make_fix(2000, 50.0, None, None));
        // Identity attitude (phone flat)
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));

        // Gravity-only accel for 1 second at 100Hz
        for i in 1..=100 {
            dr.push_accel(&make_accel(2000 + i * 10, 0.0, 0.0, GRAVITY_MS2));
        }

        let traj = dr.finalize();
        assert_eq!(traj[0].source, DrSource::Gnss);

        // DR points should stay at anchor (stationary)
        for p in traj.iter().filter(|p| p.source == DrSource::DeadReckoning) {
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
        let mut dr = DeadReckoning::new(DrConfig::default());

        dr.push_fix(&make_fix(1000, 5.0, Some(0.0), None));
        dr.push_fix(&make_fix(2000, 50.0, None, None)); // DR starts
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));
        dr.push_accel(&make_accel(2010, 0.0, 0.0, GRAVITY_MS2));
        dr.push_accel(&make_accel(2020, 0.0, 0.0, GRAVITY_MS2));
        dr.push_fix(&make_fix(3000, 5.0, Some(0.0), None)); // DR ends

        // Accel after good fix → no DR
        dr.push_accel(&make_accel(3010, 0.0, 0.0, GRAVITY_MS2));

        let traj = dr.finalize();
        let gnss: Vec<_> = traj.iter().filter(|p| p.source == DrSource::Gnss).collect();
        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == DrSource::DeadReckoning)
            .collect();

        assert_eq!(gnss.len(), 2);
        assert_eq!(dr_pts.len(), 2);
    }

    #[test]
    fn dr_max_duration() {
        let config = DrConfig {
            max_dr_duration_ms: 100,
            ..DrConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_fix(&make_fix(1000, 5.0, Some(0.0), None));
        dr.push_fix(&make_fix(2000, 50.0, None, None));
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));

        dr.push_accel(&make_accel(2050, 0.0, 0.0, GRAVITY_MS2)); // within
        dr.push_accel(&make_accel(2200, 0.0, 0.0, GRAVITY_MS2)); // beyond

        let traj = dr.finalize();
        let dr_pts: Vec<_> = traj
            .iter()
            .filter(|p| p.source == DrSource::DeadReckoning)
            .collect();
        assert_eq!(dr_pts.len(), 1);
    }

    #[test]
    fn dr_no_anchor_no_dr() {
        let mut dr = DeadReckoning::new(DrConfig::default());

        // Bad fix without prior good fix → no DR
        dr.push_fix(&make_fix(1000, 50.0, None, None));
        dr.push_attitude(&make_grv(1000, 0.0, 0.0, 0.0, 1.0));
        dr.push_accel(&make_accel(1010, 0.0, 0.0, GRAVITY_MS2));

        assert!(dr.finalize().is_empty());
    }

    #[test]
    fn dr_constant_velocity_eastward() {
        let mut dr = DeadReckoning::new(DrConfig {
            zupt_speed_threshold_mps: 0.0, // disable ZUPT
            ..DrConfig::default()
        });

        // 10 m/s East
        dr.push_fix(&make_fix(1000, 5.0, Some(10.0), Some(90.0)));
        dr.push_fix(&make_fix(2000, 50.0, None, None));
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));

        // 1 second of gravity-only (no linear accel), velocity should persist
        for i in 1..=100 {
            dr.push_accel(&make_accel(2000 + i * 10, 0.0, 0.0, GRAVITY_MS2));
        }

        let traj = dr.finalize();
        let last_dr = traj
            .iter()
            .filter(|p| p.source == DrSource::DeadReckoning)
            .last()
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
        let config = DrConfig {
            zupt_speed_threshold_mps: 0.5,
            ..DrConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_fix(&make_fix(1000, 5.0, Some(0.0), None));
        dr.push_fix(&make_fix(2000, 50.0, None, None));
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));

        for i in 1..=200 {
            dr.push_accel(&make_accel(2000 + i * 10, 0.0, 0.0, GRAVITY_MS2));
        }

        let traj = dr.finalize();
        for p in traj.iter().filter(|p| p.source == DrSource::DeadReckoning) {
            assert!(approx_eq(p.latitude_deg, 36.0, 1e-5));
            assert!(approx_eq(p.longitude_deg, 140.0, 1e-5));
        }
    }

    #[test]
    fn dr_large_gap_resets_velocity() {
        let config = DrConfig {
            zupt_speed_threshold_mps: 0.0,
            max_dt_s: 0.5,
            ..DrConfig::default()
        };
        let mut dr = DeadReckoning::new(config);

        dr.push_fix(&make_fix(1000, 5.0, Some(10.0), Some(90.0))); // 10 m/s East
        dr.push_fix(&make_fix(2000, 50.0, None, None));
        dr.push_attitude(&make_grv(2000, 0.0, 0.0, 0.0, 1.0));

        // Moving east
        for i in 1..=10 {
            dr.push_accel(&make_accel(2000 + i * 10, 0.0, 0.0, GRAVITY_MS2));
        }

        // 2-second gap → velocity reset (no point emitted for this)
        dr.push_accel(&make_accel(4100, 0.0, 0.0, GRAVITY_MS2));

        // After reset: stationary
        for i in 1..=10 {
            dr.push_accel(&make_accel(4100 + i * 10, 0.0, 0.0, GRAVITY_MS2));
        }

        let traj = dr.finalize();
        let after: Vec<_> = traj
            .iter()
            .filter(|p| p.source == DrSource::DeadReckoning && p.time_ms > 4100)
            .collect();

        // Post-gap points should be nearly stationary relative to each other
        if after.len() >= 2 {
            let spread = (after.last().unwrap().longitude_deg
                - after.first().unwrap().longitude_deg)
                .abs();
            assert!(spread < 1e-6, "should be stationary after gap, spread={spread}");
        }
    }

    #[test]
    fn dr_push_dispatches() {
        let mut dr = DeadReckoning::new(DrConfig::default());

        dr.push(&Record::Fix(make_fix(1000, 5.0, Some(0.0), None)));
        dr.push(&Record::Fix(make_fix(2000, 50.0, None, None)));
        dr.push(&Record::GameRotationVector(make_grv(2000, 0.0, 0.0, 0.0, 1.0)));
        dr.push(&Record::UncalAccel(make_accel(2010, 0.0, 0.0, GRAVITY_MS2)));
        dr.push(&Record::Skipped); // ignored

        let traj = dr.finalize();
        assert_eq!(traj.len(), 2); // 1 GNSS + 1 DR
    }
}
