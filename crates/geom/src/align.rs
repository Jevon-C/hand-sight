//! Rigid alignment of two paired 3D point sets (the Kabsch algorithm).

use nalgebra::{Isometry3, Matrix3, Point3, Rotation3, Translation3, UnitQuaternion, Vector3};

use crate::error::GeomError;

/// Smallest ratio of the second to the first singular value for the points to count as
/// spanning a plane. Below this the points are treated as lying on a line.
const MIN_SPREAD: f64 = 1e-9;

/// Find the proper rigid transform (rotation and translation, no scaling or reflection)
/// that best maps `source` onto `target` in the least-squares sense.
///
/// The points are paired by position: `source[i]` corresponds to `target[i]`.
///
/// # Errors
///
/// - [`GeomError::LengthMismatch`] if the slices differ in length.
/// - [`GeomError::TooFewPoints`] if there are fewer than three pairs.
/// - [`GeomError::Degenerate`] if the points are all on one line or all identical, because
///   the rotation about that line is then undetermined.
/// - [`GeomError::NoConvergence`] if the decomposition fails or the input is not finite.
pub fn rigid_align(
    source: &[Point3<f64>],
    target: &[Point3<f64>],
) -> Result<Isometry3<f64>, GeomError> {
    if source.len() != target.len() {
        return Err(GeomError::LengthMismatch);
    }
    if source.len() < 3 {
        return Err(GeomError::TooFewPoints);
    }
    let n = source.len() as f64;
    let source_centroid = centroid(source, n);
    let target_centroid = centroid(target, n);

    let mut covariance = Matrix3::zeros();
    for (s, t) in source.iter().zip(target) {
        covariance += (s - source_centroid) * (t - target_centroid).transpose();
    }
    if covariance.iter().any(|v| !v.is_finite()) {
        return Err(GeomError::NoConvergence);
    }

    let svd = covariance.svd(true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else {
        return Err(GeomError::NoConvergence);
    };
    let singular = svd.singular_values;
    // Singular values are sorted largest first.
    if singular[0] <= 0.0 || singular[1] < MIN_SPREAD * singular[0] {
        return Err(GeomError::Degenerate);
    }

    // Flip the last axis if needed so the result is a rotation, not a reflection.
    let v = v_t.transpose();
    let sign = (v * u.transpose()).determinant().signum();
    let correction = Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, sign));
    let rotation = v * correction * u.transpose();

    let rotation =
        UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(rotation));
    let translation = target_centroid.coords - rotation * source_centroid.coords;
    Ok(Isometry3::from_parts(
        Translation3::from(translation),
        rotation,
    ))
}

/// Root-mean-square distance between `transform * source[i]` and `target[i]`.
///
/// # Errors
///
/// [`GeomError::LengthMismatch`] or [`GeomError::EmptyInput`] for bad input shapes.
pub fn alignment_rms(
    transform: &Isometry3<f64>,
    source: &[Point3<f64>],
    target: &[Point3<f64>],
) -> Result<f64, GeomError> {
    if source.len() != target.len() {
        return Err(GeomError::LengthMismatch);
    }
    if source.is_empty() {
        return Err(GeomError::EmptyInput);
    }
    let sum: f64 = source
        .iter()
        .zip(target)
        .map(|(s, t)| (transform * s - t).norm_squared())
        .sum();
    Ok((sum / source.len() as f64).sqrt())
}

fn centroid(points: &[Point3<f64>], n: f64) -> Point3<f64> {
    Point3::from(points.iter().map(|p| p.coords).sum::<Vector3<f64>>() / n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn cloud() -> Vec<Point3<f64>> {
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.3, 0.4, 1.5),
            Point3::new(-0.7, 0.9, 0.2),
        ]
    }

    #[test]
    fn recovers_a_known_transform() -> Result<(), GeomError> {
        let truth = Isometry3::from_parts(
            Translation3::new(0.5, -1.0, 2.0),
            UnitQuaternion::from_euler_angles(0.3, -0.5, 1.1),
        );
        let source = cloud();
        let target: Vec<_> = source.iter().map(|p| truth * p).collect();
        let found = rigid_align(&source, &target)?;
        assert!(alignment_rms(&found, &source, &target)? < 1e-12);
        assert!(found.rotation.angle_to(&truth.rotation) < 1e-9);
        Ok(())
    }

    #[test]
    fn mirrored_target_is_not_matched_by_a_reflection() -> Result<(), GeomError> {
        let source = cloud();
        let target: Vec<_> = source.iter().map(|p| Point3::new(-p.x, p.y, p.z)).collect();
        let found = rigid_align(&source, &target)?;
        // A proper rotation always has determinant +1.
        assert!((found.rotation.to_rotation_matrix().matrix().determinant() - 1.0).abs() < 1e-9);
        // The mirror image cannot be matched exactly, so the residual stays positive.
        assert!(alignment_rms(&found, &source, &target)? > 1e-3);
        Ok(())
    }

    #[test]
    fn rejects_bad_input() {
        let a = cloud();
        assert_eq!(rigid_align(&a, &a[..3]), Err(GeomError::LengthMismatch));
        assert_eq!(rigid_align(&a[..2], &a[..2]), Err(GeomError::TooFewPoints));
        let line = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(2.0, 2.0, 2.0),
        ];
        assert_eq!(rigid_align(&line, &line), Err(GeomError::Degenerate));
        let same = [Point3::new(1.0, 1.0, 1.0); 4];
        assert_eq!(rigid_align(&same, &same), Err(GeomError::Degenerate));
        let bad = [
            Point3::new(f64::NAN, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        assert_eq!(rigid_align(&bad, &bad), Err(GeomError::NoConvergence));
    }

    #[test]
    fn rms_rejects_bad_shapes() {
        let a = cloud();
        let id = Isometry3::identity();
        assert_eq!(
            alignment_rms(&id, &a, &a[..2]),
            Err(GeomError::LengthMismatch)
        );
        assert_eq!(alignment_rms(&id, &[], &[]), Err(GeomError::EmptyInput));
    }

    proptest! {
        #[test]
        fn round_trips_random_transforms(
            roll in -3.0..3.0f64, pitch in -1.5..1.5f64, yaw in -3.0..3.0f64,
            tx in -5.0..5.0f64, ty in -5.0..5.0f64, tz in -5.0..5.0f64,
        ) {
            let truth = Isometry3::from_parts(
                Translation3::new(tx, ty, tz),
                UnitQuaternion::from_euler_angles(roll, pitch, yaw),
            );
            let source = cloud();
            let target: Vec<_> = source.iter().map(|p| truth * p).collect();
            let found = rigid_align(&source, &target).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let rms = alignment_rms(&found, &source, &target).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!(rms < 1e-9);
        }
    }
}
