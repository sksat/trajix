//! Geodesic utility functions.
//!
//! Provides distance and bearing calculations for WGS84 coordinates.

const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Haversine distance between two WGS84 coordinates in meters.
///
/// Accuracy: ~0.5% for distances under 1000 km on Earth's surface.
///
/// # Example
/// ```
/// use trajix::geo::haversine_distance_m;
///
/// // Tokyo Station to Osaka Station: ~402 km
/// let dist = haversine_distance_m(35.6812, 139.7671, 34.7024, 135.4959);
/// assert!((dist - 402_000.0).abs() < 5000.0);
/// ```
pub fn haversine_distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1r = lat1.to_radians();
    let lat2r = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1r.cos() * lat2r.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_RADIUS_M * c
}

/// Initial bearing (forward azimuth) from point 1 to point 2 in degrees [0, 360).
///
/// Returns the compass bearing at the starting point to follow a great circle
/// to the destination.
///
/// # Example
/// ```
/// use trajix::geo::bearing_deg;
///
/// // Due east
/// let b = bearing_deg(0.0, 0.0, 0.0, 1.0);
/// assert!((b - 90.0).abs() < 0.01);
/// ```
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1r = lat1.to_radians();
    let lat2r = lat2.to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let y = dlon.sin() * lat2r.cos();
    let x = lat1r.cos() * lat2r.sin() - lat1r.sin() * lat2r.cos() * dlon.cos();

    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_tokyo_to_osaka() {
        // Tokyo Station → Osaka Station ≈ 402 km
        let dist = haversine_distance_m(35.6812, 139.7671, 34.7024, 135.4959);
        assert!(
            (dist - 402_500.0).abs() < 5000.0,
            "expected ~402km, got {:.0}m",
            dist
        );
    }

    #[test]
    fn haversine_short_distance() {
        // Two points ~111m apart (0.001 degrees latitude ≈ 111m)
        let dist = haversine_distance_m(36.0, 140.0, 36.001, 140.0);
        assert!(
            (dist - 111.0).abs() < 2.0,
            "expected ~111m, got {:.1}m",
            dist
        );
    }

    #[test]
    fn haversine_same_point() {
        let dist = haversine_distance_m(36.212, 140.097, 36.212, 140.097);
        assert!(dist.abs() < 1e-10, "same point should be 0m, got {dist}");
    }

    #[test]
    fn haversine_antipodal() {
        // North pole to south pole ≈ half circumference ≈ 20,015 km
        let dist = haversine_distance_m(90.0, 0.0, -90.0, 0.0);
        assert!(
            (dist - 20_015_000.0).abs() < 100_000.0,
            "expected ~20,015km, got {:.0}m",
            dist
        );
    }

    #[test]
    fn haversine_equator_90_degrees() {
        // 90 degrees along equator ≈ quarter circumference ≈ 10,008 km
        let dist = haversine_distance_m(0.0, 0.0, 0.0, 90.0);
        assert!(
            (dist - 10_008_000.0).abs() < 50_000.0,
            "expected ~10,008km, got {:.0}m",
            dist
        );
    }

    #[test]
    fn bearing_due_north() {
        let b = bearing_deg(0.0, 0.0, 1.0, 0.0);
        assert!((b - 0.0).abs() < 0.01, "expected 0°, got {b:.2}°");
    }

    #[test]
    fn bearing_due_east() {
        let b = bearing_deg(0.0, 0.0, 0.0, 1.0);
        assert!((b - 90.0).abs() < 0.01, "expected 90°, got {b:.2}°");
    }

    #[test]
    fn bearing_due_south() {
        let b = bearing_deg(1.0, 0.0, 0.0, 0.0);
        assert!((b - 180.0).abs() < 0.01, "expected 180°, got {b:.2}°");
    }

    #[test]
    fn bearing_due_west() {
        let b = bearing_deg(0.0, 1.0, 0.0, 0.0);
        assert!((b - 270.0).abs() < 0.01, "expected 270°, got {b:.2}°");
    }

    #[test]
    fn bearing_same_point() {
        // Bearing is undefined for same point, but should not panic
        let _ = bearing_deg(36.212, 140.097, 36.212, 140.097);
    }

    #[test]
    fn bearing_tokyo_to_osaka() {
        // Tokyo → Osaka is roughly west-southwest ≈ 256°
        let b = bearing_deg(35.6812, 139.7671, 34.7024, 135.4959);
        assert!((b - 256.0).abs() < 5.0, "expected ~256°, got {b:.1}°");
    }
}
