//! Camera pose from a flat target with known layout, such as a calibration board.

use nalgebra::{
    Isometry3, Matrix3, Point2, Rotation3, SMatrix, SVector, Translation3, UnitQuaternion, Vector2,
    Vector3,
};

use crate::camera::Camera;
use crate::error::GeomError;

/// Minimum number of point pairs needed to determine a plane-to-image mapping.
const MIN_POINTS: usize = 4;
/// Smallest ratio of the second smallest to the largest eigenvalue of the system for the
/// target points to count as spread over a plane.
const MIN_SPREAD: f64 = 1e-12;
const REFINE_ITERATIONS: usize = 30;
const STEP_EPS: f64 = 1e-7;
const FINITE_DIFF: f64 = 1e-6;

/// Estimate where a flat target is relative to the camera.
///
/// `board[i]` is the position of marker `i` on the target, in metres, in the target's own
/// plane (the target lies at z = 0). `pixels[i]` is where that marker was seen in the image.
/// The result maps target coordinates `(x, y, 0)` into the camera frame (+z forward), so it is
/// the "camera from board" transform.
///
/// The method is a direct linear solve of the plane-to-image mapping, followed by a few
/// damped Gauss-Newton steps that minimise the reprojection error. Lens distortion is removed
/// first using the camera's own model.
///
/// # Errors
///
/// - [`GeomError::LengthMismatch`] if the slices differ in length.
/// - [`GeomError::TooFewPoints`] for fewer than four pairs.
/// - [`GeomError::Degenerate`] if the target points lie on a line, or the target faces
///   edge-on so that no unique pose exists.
/// - [`GeomError::NoConvergence`] if undistortion fails or the input is not finite.
pub fn pose_from_planar_points(
    camera: &Camera,
    board: &[Point2<f64>],
    pixels: &[Point2<f64>],
) -> Result<Isometry3<f64>, GeomError> {
    if board.len() != pixels.len() {
        return Err(GeomError::LengthMismatch);
    }
    if board.len() < MIN_POINTS {
        return Err(GeomError::TooFewPoints);
    }
    let normalised = pixels
        .iter()
        .map(|px| {
            let p = camera.unproject(px, 1.0)?;
            Ok(Point2::new(p.x, p.y))
        })
        .collect::<Result<Vec<_>, GeomError>>()?;
    if board.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(GeomError::NoConvergence);
    }

    let h = homography(board, &normalised)?;
    let initial = pose_from_homography(&h)?;
    let weight = 0.5 * (camera.intrinsics.fx + camera.intrinsics.fy);
    Ok(refine(initial, board, &normalised, weight))
}

/// Mapping from board coordinates to normalised image coordinates, up to scale, by the
/// direct linear method with Hartley conditioning of both point sets.
fn homography(board: &[Point2<f64>], image: &[Point2<f64>]) -> Result<Matrix3<f64>, GeomError> {
    let (t_board, board_n) = condition(board);
    let (t_image, image_n) = condition(image);

    let mut ata = SMatrix::<f64, 9, 9>::zeros();
    for (b, i) in board_n.iter().zip(&image_n) {
        let row_u = SVector::<f64, 9>::from_row_slice(&[
            -b.x,
            -b.y,
            -1.0,
            0.0,
            0.0,
            0.0,
            i.x * b.x,
            i.x * b.y,
            i.x,
        ]);
        let row_v = SVector::<f64, 9>::from_row_slice(&[
            0.0,
            0.0,
            0.0,
            -b.x,
            -b.y,
            -1.0,
            i.y * b.x,
            i.y * b.y,
            i.y,
        ]);
        ata += row_u * row_u.transpose() + row_v * row_v.transpose();
    }
    if ata.iter().any(|v| !v.is_finite()) {
        return Err(GeomError::NoConvergence);
    }
    let eigen = ata.symmetric_eigen();
    let mut order: Vec<usize> = (0..9).collect();
    order.sort_by(|&a, &b| eigen.eigenvalues[a].total_cmp(&eigen.eigenvalues[b]));
    let (smallest, second, largest) = (order[0], order[1], order[8]);
    if eigen.eigenvalues[largest] <= 0.0
        || eigen.eigenvalues[second] < MIN_SPREAD * eigen.eigenvalues[largest]
    {
        return Err(GeomError::Degenerate);
    }
    let v = eigen.eigenvectors.column(smallest);
    let hn = Matrix3::new(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8]);
    let t_image_inv = t_image.try_inverse().ok_or(GeomError::NoConvergence)?;
    Ok(t_image_inv * hn * t_board)
}

/// Similarity transform moving the centroid to the origin and the mean distance to sqrt(2),
/// with the transformed points.
fn condition(points: &[Point2<f64>]) -> (Matrix3<f64>, Vec<Point2<f64>>) {
    let n = points.len() as f64;
    let mean = points.iter().map(|p| p.coords).sum::<Vector2<f64>>() / n;
    let spread = points.iter().map(|p| (p.coords - mean).norm()).sum::<f64>() / n;
    let scale = if spread > 0.0 {
        std::f64::consts::SQRT_2 / spread
    } else {
        1.0
    };
    let t = Matrix3::new(
        scale,
        0.0,
        -scale * mean.x,
        0.0,
        scale,
        -scale * mean.y,
        0.0,
        0.0,
        1.0,
    );
    let moved = points
        .iter()
        .map(|p| Point2::from((p.coords - mean) * scale))
        .collect();
    (t, moved)
}

/// Split the homography into a rotation and translation for a target at z = 0.
fn pose_from_homography(h: &Matrix3<f64>) -> Result<Isometry3<f64>, GeomError> {
    let (h1, h2, h3) = (h.column(0), h.column(1), h.column(2));
    let norm = 0.5 * (h1.norm() + h2.norm());
    if norm <= 0.0 || !norm.is_finite() {
        return Err(GeomError::Degenerate);
    }
    let mut lambda = 1.0 / norm;
    // The target must be in front of the camera.
    if lambda * h3[2] < 0.0 {
        lambda = -lambda;
    }
    let r1 = h1 * lambda;
    let r2 = h2 * lambda;
    let r3 = r1.cross(&r2);
    let approx = Matrix3::from_columns(&[r1, r2, r3]);
    // Nearest true rotation to the noisy estimate.
    let svd = approx.svd(true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else {
        return Err(GeomError::NoConvergence);
    };
    let sign = (u * v_t).determinant().signum();
    let rotation = u * Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, sign)) * v_t;
    let translation = h3 * lambda;
    if translation[2] <= 0.0 || !translation[2].is_finite() {
        return Err(GeomError::Degenerate);
    }
    Ok(Isometry3::from_parts(
        Translation3::from(Vector3::from(translation)),
        UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(rotation)),
    ))
}

/// Damped Gauss-Newton on the reprojection error in normalised coordinates, scaled by
/// `weight` so residuals read roughly as pixels.
fn refine(
    start: Isometry3<f64>,
    board: &[Point2<f64>],
    image: &[Point2<f64>],
    weight: f64,
) -> Isometry3<f64> {
    let mut pose = start;
    let mut damping = 1e-3;
    let mut cost = residual_cost(&pose, board, image, weight);
    for _ in 0..REFINE_ITERATIONS {
        let r = residuals(&pose, board, image, weight);
        let jacobian = numeric_jacobian(&pose, board, image, weight);
        let jt = jacobian.transpose();
        let gradient = &jt * &r;
        let mut normal = &jt * &jacobian;
        for k in 0..6 {
            normal[(k, k)] += damping;
        }
        let Some(step) = normal.lu().solve(&(-gradient)) else {
            break;
        };
        let step = SVector::<f64, 6>::from_iterator(step.iter().copied());
        let candidate = apply_step(&pose, &step);
        let candidate_cost = residual_cost(&candidate, board, image, weight);
        if candidate_cost < cost {
            pose = candidate;
            cost = candidate_cost;
            damping = (damping * 0.3).max(1e-12);
            if step.norm() < STEP_EPS {
                break;
            }
        } else {
            damping *= 10.0;
            if damping > 1e12 {
                break;
            }
        }
    }
    pose
}

type Residuals = nalgebra::DVector<f64>;

fn residuals(
    pose: &Isometry3<f64>,
    board: &[Point2<f64>],
    image: &[Point2<f64>],
    weight: f64,
) -> Residuals {
    let mut out = Residuals::zeros(2 * board.len());
    for (k, (b, i)) in board.iter().zip(image).enumerate() {
        let p = pose * nalgebra::Point3::new(b.x, b.y, 0.0);
        // A point at or behind the camera plane gets a large residual instead of an error,
        // so the solver backs away from it.
        let (u, v) = if p.z > 1e-9 {
            (p.x / p.z, p.y / p.z)
        } else {
            (1e6, 1e6)
        };
        out[2 * k] = weight * (u - i.x);
        out[2 * k + 1] = weight * (v - i.y);
    }
    out
}

fn residual_cost(
    pose: &Isometry3<f64>,
    board: &[Point2<f64>],
    image: &[Point2<f64>],
    weight: f64,
) -> f64 {
    residuals(pose, board, image, weight).norm_squared()
}

fn apply_step(pose: &Isometry3<f64>, step: &SVector<f64, 6>) -> Isometry3<f64> {
    let turn = UnitQuaternion::from_scaled_axis(Vector3::new(step[0], step[1], step[2]));
    let shift = Vector3::new(step[3], step[4], step[5]);
    Isometry3::from_parts(
        Translation3::from(pose.translation.vector + shift),
        turn * pose.rotation,
    )
}

fn numeric_jacobian(
    pose: &Isometry3<f64>,
    board: &[Point2<f64>],
    image: &[Point2<f64>],
    weight: f64,
) -> nalgebra::DMatrix<f64> {
    let mut j = nalgebra::DMatrix::zeros(2 * board.len(), 6);
    for k in 0..6 {
        let mut up = SVector::<f64, 6>::zeros();
        up[k] = FINITE_DIFF;
        let plus = residuals(&apply_step(pose, &up), board, image, weight);
        let minus = residuals(&apply_step(pose, &(-up)), board, image, weight);
        j.set_column(k, &((plus - minus) / (2.0 * FINITE_DIFF)));
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Distortion, Pinhole};
    use nalgebra::Point3;

    const CAM: Camera = Camera {
        intrinsics: Pinhole {
            fx: 900.0,
            fy: 905.0,
            cx: 640.0,
            cy: 360.0,
        },
        distortion: Distortion {
            k1: -0.12,
            k2: 0.03,
            p1: 0.001,
            p2: -0.0005,
            k3: 0.0,
        },
    };

    fn board() -> Vec<Point2<f64>> {
        let mut out = Vec::new();
        for i in 0..6 {
            for j in 0..4 {
                out.push(Point2::new(
                    f64::from(i) * 0.03 - 0.075,
                    f64::from(j) * 0.03 - 0.045,
                ));
            }
        }
        out
    }

    fn view(pose: &Isometry3<f64>) -> Result<Vec<Point2<f64>>, GeomError> {
        board()
            .iter()
            .map(|b| CAM.project(&(pose * Point3::new(b.x, b.y, 0.0))))
            .collect()
    }

    fn truth() -> Isometry3<f64> {
        Isometry3::from_parts(
            Translation3::new(0.02, -0.01, 0.45),
            UnitQuaternion::from_euler_angles(0.4, -0.3, 0.2),
        )
    }

    #[test]
    fn recovers_pose_from_exact_observations() -> Result<(), GeomError> {
        let truth = truth();
        let found = pose_from_planar_points(&CAM, &board(), &view(&truth)?)?;
        assert!(found.rotation.angle_to(&truth.rotation) < 1e-6);
        assert!((found.translation.vector - truth.translation.vector).norm() < 1e-6);
        Ok(())
    }

    #[test]
    fn tolerates_pixel_noise() -> Result<(), GeomError> {
        let truth = truth();
        let mut pixels = view(&truth)?;
        // Deterministic, zero-mean jitter of about half a pixel.
        for (k, p) in pixels.iter_mut().enumerate() {
            let a = (k as f64 * 2.399_963).sin() * 0.5;
            let b = (k as f64 * 1.234_567).cos() * 0.5;
            *p += Vector2::new(a, b);
        }
        let found = pose_from_planar_points(&CAM, &board(), &pixels)?;
        let world: Vec<_> = board().iter().map(|b| Point3::new(b.x, b.y, 0.0)).collect();
        assert!(CAM.reprojection_rms(&found, &world, &pixels)? < 0.8);
        assert!((found.translation.vector - truth.translation.vector).norm() < 0.005);
        Ok(())
    }

    #[test]
    fn rejects_bad_input() -> Result<(), GeomError> {
        let pixels = view(&truth())?;
        let b = board();
        assert_eq!(
            pose_from_planar_points(&CAM, &b, &pixels[..5]),
            Err(GeomError::LengthMismatch)
        );
        assert_eq!(
            pose_from_planar_points(&CAM, &b[..3], &pixels[..3]),
            Err(GeomError::TooFewPoints)
        );
        let line: Vec<_> = (0..6)
            .map(|i| Point2::new(f64::from(i) * 0.03, 0.0))
            .collect();
        let line_pixels: Vec<_> = line
            .iter()
            .map(|p| CAM.project(&(truth() * Point3::new(p.x, p.y, 0.0))))
            .collect::<Result<_, _>>()?;
        assert_eq!(
            pose_from_planar_points(&CAM, &line, &line_pixels),
            Err(GeomError::Degenerate)
        );
        let mut nan = b.clone();
        nan[0].x = f64::NAN;
        assert_eq!(
            pose_from_planar_points(&CAM, &nan, &pixels),
            Err(GeomError::NoConvergence)
        );
        Ok(())
    }
}
