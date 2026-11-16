//! Plane fitting: least squares, and RANSAC for data with outliers.

use nalgebra::{Matrix3, Point3, Unit, Vector3};

use crate::error::GeomError;

/// Smallest ratio of the middle to the largest covariance eigenvalue for the points to
/// count as spreading over a plane rather than lying on a line.
const MIN_SPREAD: f64 = 1e-12;

/// A plane described by a unit normal `n` and an offset `d`: points `p` on it satisfy
/// `n . p = d`. The sign of the normal is arbitrary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Unit<Vector3<f64>>,
    /// Offset along the normal from the origin.
    pub offset: f64,
}

impl Plane {
    /// Signed distance from `p` to the plane, positive on the side the normal points to.
    #[must_use]
    pub fn signed_distance(&self, p: &Point3<f64>) -> f64 {
        self.normal.dot(&p.coords) - self.offset
    }
}

/// Result of [`fit_plane_ransac`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneFit {
    /// The plane, refitted by least squares to all inliers.
    pub plane: Plane,
    /// Indices into the input of the points within the threshold of the plane.
    pub inliers: Vec<usize>,
}

/// Least-squares plane through `points`.
///
/// # Errors
///
/// - [`GeomError::TooFewPoints`] for fewer than three points.
/// - [`GeomError::Degenerate`] if the points lie on a line or coincide.
/// - [`GeomError::NoConvergence`] if any coordinate is not finite.
pub fn fit_plane(points: &[Point3<f64>]) -> Result<Plane, GeomError> {
    if points.len() < 3 {
        return Err(GeomError::TooFewPoints);
    }
    let n = points.len() as f64;
    let mean = points.iter().map(|p| p.coords).sum::<Vector3<f64>>() / n;
    let mut covariance = Matrix3::zeros();
    for p in points {
        let d = p.coords - mean;
        covariance += d * d.transpose();
    }
    if covariance.iter().any(|v| !v.is_finite()) {
        return Err(GeomError::NoConvergence);
    }
    let eigen = covariance.symmetric_eigen();
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| eigen.eigenvalues[a].total_cmp(&eigen.eigenvalues[b]));
    let [smallest, middle, largest] = order;
    if eigen.eigenvalues[largest] <= 0.0
        || eigen.eigenvalues[middle] < MIN_SPREAD * eigen.eigenvalues[largest]
    {
        return Err(GeomError::Degenerate);
    }
    let normal = Unit::new_normalize(eigen.eigenvectors.column(smallest).into_owned());
    Ok(Plane {
        normal,
        offset: normal.dot(&mean),
    })
}

/// Robust plane fit: repeatedly fit a plane through three random points, keep the plane
/// with the most points within `threshold`, then refit to those points.
///
/// The random choices come from a small generator seeded with `seed`, so the same inputs
/// always give the same answer. Ties go to the earliest candidate.
///
/// # Errors
///
/// - [`GeomError::InvalidParameter`] if `threshold` is not positive and finite or
///   `iterations` is zero.
/// - [`GeomError::TooFewPoints`] for fewer than three points.
/// - [`GeomError::Degenerate`] if no three-point sample spans a plane, or the inliers do.
pub fn fit_plane_ransac(
    points: &[Point3<f64>],
    threshold: f64,
    iterations: usize,
    seed: u64,
) -> Result<PlaneFit, GeomError> {
    if !(threshold.is_finite() && threshold > 0.0) || iterations == 0 {
        return Err(GeomError::InvalidParameter);
    }
    if points.len() < 3 {
        return Err(GeomError::TooFewPoints);
    }
    let mut rng = SplitMix64(seed);
    let mut best: Option<Vec<usize>> = None;
    for _ in 0..iterations {
        let [a, b, c] = pick_three(&mut rng, points.len());
        let Ok(candidate) = fit_plane(&[points[a], points[b], points[c]]) else {
            continue;
        };
        let inliers = inlier_indices(&candidate, points, threshold);
        if best.as_ref().is_none_or(|kept| inliers.len() > kept.len()) {
            best = Some(inliers);
        }
    }
    let inliers = best.ok_or(GeomError::Degenerate)?;
    let chosen: Vec<_> = inliers.iter().map(|&i| points[i]).collect();
    let plane = fit_plane(&chosen)?;
    // Refitting can move the plane, so recount against the final one.
    let inliers = inlier_indices(&plane, points, threshold);
    Ok(PlaneFit { plane, inliers })
}

fn inlier_indices(plane: &Plane, points: &[Point3<f64>], threshold: f64) -> Vec<usize> {
    points
        .iter()
        .enumerate()
        .filter(|(_, p)| plane.signed_distance(p).abs() <= threshold)
        .map(|(i, _)| i)
        .collect()
}

/// Three distinct indices below `len` (which is at least 3).
fn pick_three(rng: &mut SplitMix64, len: usize) -> [usize; 3] {
    let a = rng.below(len);
    let mut b = rng.below(len - 1);
    if b >= a {
        b += 1;
    }
    let mut c = rng.below(len - 2);
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if c >= lo {
        c += 1;
    }
    if c >= hi {
        c += 1;
    }
    [a, b, c]
}

/// Small deterministic generator (SplitMix64); enough for sampling, not for security.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..bound` (bound must be positive). The tiny bias is irrelevant here.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Grid of points on the plane z = 0.5 x + 0.25 y + 2.
    fn tilted_grid() -> Vec<Point3<f64>> {
        let mut out = Vec::new();
        for i in 0..8 {
            for j in 0..8 {
                let (x, y) = (f64::from(i) * 0.1, f64::from(j) * 0.1);
                out.push(Point3::new(x, y, 0.5 * x + 0.25 * y + 2.0));
            }
        }
        out
    }

    #[test]
    fn least_squares_fits_an_exact_plane() -> Result<(), GeomError> {
        let points = tilted_grid();
        let plane = fit_plane(&points)?;
        assert!(points
            .iter()
            .all(|p| plane.signed_distance(p).abs() < 1e-12));
        Ok(())
    }

    #[test]
    fn least_squares_rejects_bad_input() {
        let p = Point3::new(0.0, 0.0, 0.0);
        assert_eq!(fit_plane(&[p, p]), Err(GeomError::TooFewPoints));
        assert_eq!(fit_plane(&[p, p, p, p]), Err(GeomError::Degenerate));
        let line = [p, Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)];
        assert_eq!(fit_plane(&line), Err(GeomError::Degenerate));
        let bad = [
            p,
            Point3::new(f64::NAN, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ];
        assert_eq!(fit_plane(&bad), Err(GeomError::NoConvergence));
    }

    #[test]
    fn ransac_ignores_outliers() -> Result<(), GeomError> {
        let mut points = tilted_grid();
        let clean = points.len();
        for k in 0..12 {
            let t = f64::from(k);
            points.push(Point3::new(0.1 * t, 0.7 - 0.05 * t, 5.0 + t));
        }
        let fit = fit_plane_ransac(&points, 1e-6, 200, 7)?;
        assert_eq!(fit.inliers, (0..clean).collect::<Vec<_>>());
        assert!(points[..clean]
            .iter()
            .all(|p| fit.plane.signed_distance(p).abs() < 1e-9));
        Ok(())
    }

    #[test]
    fn ransac_is_deterministic_for_a_seed() -> Result<(), GeomError> {
        let mut points = tilted_grid();
        points.push(Point3::new(0.2, 0.2, 9.0));
        let first = fit_plane_ransac(&points, 1e-6, 50, 3)?;
        let second = fit_plane_ransac(&points, 1e-6, 50, 3)?;
        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn ransac_rejects_bad_parameters() {
        let points = tilted_grid();
        assert_eq!(
            fit_plane_ransac(&points, 0.0, 10, 1),
            Err(GeomError::InvalidParameter)
        );
        assert_eq!(
            fit_plane_ransac(&points, f64::NAN, 10, 1),
            Err(GeomError::InvalidParameter)
        );
        assert_eq!(
            fit_plane_ransac(&points, 0.1, 0, 1),
            Err(GeomError::InvalidParameter)
        );
        assert_eq!(
            fit_plane_ransac(&points[..2], 0.1, 10, 1),
            Err(GeomError::TooFewPoints)
        );
        let same = [Point3::new(1.0, 1.0, 1.0); 5];
        assert_eq!(
            fit_plane_ransac(&same, 0.1, 10, 1),
            Err(GeomError::Degenerate)
        );
    }

    proptest! {
        #[test]
        fn picked_indices_are_distinct_and_in_range(seed in any::<u64>(), len in 3usize..40) {
            let mut rng = SplitMix64(seed);
            let [a, b, c] = pick_three(&mut rng, len);
            prop_assert!(a < len && b < len && c < len);
            prop_assert!(a != b && a != c && b != c);
        }
    }
}
