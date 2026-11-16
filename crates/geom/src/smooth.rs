//! Smoothing of noisy tracks with a Kalman filter and a Rauch-Tung-Striebel backward pass.

use nalgebra::{Matrix2, Point3, Vector2};

use crate::error::GeomError;

/// Settings for [`smooth_track`].
///
/// The track is modelled as a position moving with a velocity that drifts randomly. A larger
/// `acceleration_variance` follows fast changes more closely; a larger `measurement_variance`
/// trusts the measurements less and smooths more.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmootherConfig {
    /// Time between samples, in seconds. Must be positive.
    pub dt: f64,
    /// Variance of the random acceleration, in track units squared per second to the fourth.
    pub acceleration_variance: f64,
    /// Variance of the measurement noise, in track units squared.
    pub measurement_variance: f64,
}

impl SmootherConfig {
    fn validate(&self) -> Result<(), GeomError> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if ok(self.dt) && ok(self.acceleration_variance) && ok(self.measurement_variance) {
            Ok(())
        } else {
            Err(GeomError::InvalidParameter)
        }
    }
}

/// Smooth one scalar track sampled at a fixed rate.
///
/// Entries that are NaN count as missing: the filter carries the track across them, so gaps
/// are filled by the model. Samples before the first real measurement hold that first value.
///
/// # Errors
///
/// - [`GeomError::InvalidParameter`] if any setting is not positive and finite.
/// - [`GeomError::EmptyInput`] if there is no finite measurement at all.
/// - [`GeomError::NoConvergence`] if a matrix in the filter cannot be inverted.
pub fn smooth_track(values: &[f64], config: &SmootherConfig) -> Result<Vec<f64>, GeomError> {
    config.validate()?;
    let Some(first) = values.iter().copied().find(|v| v.is_finite()) else {
        return Err(GeomError::EmptyInput);
    };
    let dt = config.dt;
    let transition = Matrix2::new(1.0, dt, 0.0, 1.0);
    let q = config.acceleration_variance;
    let process = Matrix2::new(
        q * dt.powi(4) / 4.0,
        q * dt.powi(3) / 2.0,
        q * dt.powi(3) / 2.0,
        q * dt * dt,
    );
    let r = config.measurement_variance;

    let n = values.len();
    let mut predicted_state = Vec::with_capacity(n);
    let mut predicted_cov = Vec::with_capacity(n);
    let mut filtered_state = Vec::with_capacity(n);
    let mut filtered_cov = Vec::with_capacity(n);

    // Wide starting uncertainty so the first real measurement dominates.
    let mut x = Vector2::new(first, 0.0);
    let mut p = Matrix2::new(1e6 * r, 0.0, 0.0, 1e6 * r / (dt * dt));
    for (k, &z) in values.iter().enumerate() {
        let (xp, pp) = if k == 0 {
            (x, p)
        } else {
            (
                transition * x,
                transition * p * transition.transpose() + process,
            )
        };
        predicted_state.push(xp);
        predicted_cov.push(pp);
        if z.is_finite() {
            let innovation = z - xp.x;
            let s = pp[(0, 0)] + r;
            let gain = Vector2::new(pp[(0, 0)] / s, pp[(1, 0)] / s);
            x = xp + gain * innovation;
            // Joseph form keeps the covariance symmetric and positive.
            let ikh = Matrix2::identity() - Matrix2::new(gain.x, 0.0, gain.y, 0.0);
            p = ikh * pp * ikh.transpose() + gain * gain.transpose() * r;
        } else {
            x = xp;
            p = pp;
        }
        filtered_state.push(x);
        filtered_cov.push(p);
    }

    let mut smoothed = filtered_state.clone();
    for k in (0..n.saturating_sub(1)).rev() {
        let inverse = predicted_cov[k + 1]
            .try_inverse()
            .ok_or(GeomError::NoConvergence)?;
        let c = filtered_cov[k] * transition.transpose() * inverse;
        smoothed[k] = filtered_state[k] + c * (smoothed[k + 1] - predicted_state[k + 1]);
    }
    let out: Vec<f64> = smoothed.iter().map(|s| s.x).collect();
    if out.iter().any(|v| !v.is_finite()) {
        return Err(GeomError::NoConvergence);
    }
    Ok(out)
}

/// Smooth a sequence of 3D points, one axis at a time with the same settings.
///
/// A point with any non-finite coordinate is treated as missing on that axis only.
///
/// # Errors
///
/// As for [`smooth_track`].
pub fn smooth_points(
    points: &[Point3<f64>],
    config: &SmootherConfig,
) -> Result<Vec<Point3<f64>>, GeomError> {
    let axis = |pick: fn(&Point3<f64>) -> f64| -> Result<Vec<f64>, GeomError> {
        let values: Vec<f64> = points.iter().map(pick).collect();
        smooth_track(&values, config)
    };
    let (x, y, z) = (axis(|p| p.x)?, axis(|p| p.y)?, axis(|p| p.z)?);
    Ok(x.into_iter()
        .zip(y)
        .zip(z)
        .map(|((x, y), z)| Point3::new(x, y, z))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: SmootherConfig = SmootherConfig {
        dt: 1.0 / 60.0,
        acceleration_variance: 50.0,
        measurement_variance: 0.01,
    };

    fn rms(a: &[f64], b: &[f64]) -> f64 {
        let sum: f64 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
        (sum / a.len() as f64).sqrt()
    }

    /// Deterministic noise with standard deviation near 0.1.
    fn noise(k: usize) -> f64 {
        0.14 * (k as f64 * 12.9898).sin()
    }

    #[test]
    fn reduces_noise_on_a_slow_signal() -> Result<(), GeomError> {
        let truth: Vec<f64> = (0..300).map(|k| (k as f64 / 60.0 * 2.0).sin()).collect();
        let noisy: Vec<f64> = truth
            .iter()
            .enumerate()
            .map(|(k, t)| t + noise(k))
            .collect();
        let smooth = smooth_track(&noisy, &CONFIG)?;
        assert!(rms(&smooth, &truth) < 0.6 * rms(&noisy, &truth));
        Ok(())
    }

    #[test]
    fn follows_a_straight_line_closely() -> Result<(), GeomError> {
        let truth: Vec<f64> = (0..120).map(|k| 0.5 + 0.02 * k as f64).collect();
        let smooth = smooth_track(&truth, &CONFIG)?;
        assert!(rms(&smooth, &truth) < 1e-3);
        Ok(())
    }

    #[test]
    fn fills_gaps_from_the_motion_model() -> Result<(), GeomError> {
        let truth: Vec<f64> = (0..120).map(|k| 0.5 + 0.02 * k as f64).collect();
        let mut gappy = truth.clone();
        for v in &mut gappy[50..60] {
            *v = f64::NAN;
        }
        let smooth = smooth_track(&gappy, &CONFIG)?;
        assert!(rms(&smooth[50..60], &truth[50..60]) < 0.01);
        Ok(())
    }

    #[test]
    fn leading_gap_holds_the_first_measurement() -> Result<(), GeomError> {
        let mut values = vec![f64::NAN; 5];
        values.extend(vec![2.0; 20]);
        let smooth = smooth_track(&values, &CONFIG)?;
        assert!(smooth.iter().all(|v| (v - 2.0).abs() < 1e-3));
        Ok(())
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(smooth_track(&[], &CONFIG), Err(GeomError::EmptyInput));
        assert_eq!(
            smooth_track(&[f64::NAN; 4], &CONFIG),
            Err(GeomError::EmptyInput)
        );
        for bad in [
            SmootherConfig { dt: 0.0, ..CONFIG },
            SmootherConfig {
                acceleration_variance: -1.0,
                ..CONFIG
            },
            SmootherConfig {
                measurement_variance: f64::NAN,
                ..CONFIG
            },
        ] {
            assert_eq!(
                smooth_track(&[1.0, 2.0], &bad),
                Err(GeomError::InvalidParameter)
            );
        }
    }

    #[test]
    fn single_sample_is_returned_unchanged() -> Result<(), GeomError> {
        assert_eq!(smooth_track(&[3.0], &CONFIG)?, vec![3.0]);
        Ok(())
    }

    #[test]
    fn smooths_each_axis_of_points() -> Result<(), GeomError> {
        let points: Vec<_> = (0..100)
            .map(|k| Point3::new(f64::from(k) * 0.01, 1.0, -0.5 + f64::from(k) * 0.02))
            .collect();
        let smooth = smooth_points(&points, &CONFIG)?;
        assert_eq!(smooth.len(), points.len());
        assert!(smooth
            .iter()
            .zip(&points)
            .all(|(a, b)| (a - b).norm() < 1e-3));
        Ok(())
    }
}
