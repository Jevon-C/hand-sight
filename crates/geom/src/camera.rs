//! Camera models: ideal pinhole, and pinhole with Brown-Conrady lens distortion.

use nalgebra::{Isometry3, Point2, Point3, Vector2, Vector3};

use crate::error::GeomError;

/// Ideal pinhole camera (no distortion) in the camera frame, +z forward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pinhole {
    /// Focal length along x, in pixels.
    pub fx: f64,
    /// Focal length along y, in pixels.
    pub fy: f64,
    /// Principal point x, in pixels.
    pub cx: f64,
    /// Principal point y, in pixels.
    pub cy: f64,
}

impl Pinhole {
    /// Project a camera-frame 3D point to pixel coordinates.
    pub fn project(&self, p: &Point3<f64>) -> Result<Point2<f64>, GeomError> {
        if p.z <= 0.0 || !p.z.is_finite() {
            return Err(GeomError::BehindCamera);
        }
        Ok(Point2::new(
            self.fx * p.x / p.z + self.cx,
            self.fy * p.y / p.z + self.cy,
        ))
    }

    /// Back-project a pixel at a known depth to a camera-frame 3D point.
    pub fn unproject(&self, px: &Point2<f64>, depth: f64) -> Result<Point3<f64>, GeomError> {
        if depth <= 0.0 || !depth.is_finite() {
            return Err(GeomError::InvalidDepth);
        }
        let v = Vector3::new(
            (px.x - self.cx) / self.fx * depth,
            (px.y - self.cy) / self.fy * depth,
            depth,
        );
        Ok(Point3::from(v))
    }
}

/// Brown-Conrady distortion: radial k1, k2, k3 and tangential p1, p2.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Distortion {
    /// First radial coefficient.
    pub k1: f64,
    /// Second radial coefficient.
    pub k2: f64,
    /// First tangential coefficient.
    pub p1: f64,
    /// Second tangential coefficient.
    pub p2: f64,
    /// Third radial coefficient.
    pub k3: f64,
}

impl Distortion {
    const MAX_ITERATIONS: usize = 100;
    const TOLERANCE: f64 = 1e-12;

    /// Apply distortion to normalised image coordinates (x/z, y/z).
    pub fn distort(&self, n: Vector2<f64>) -> Vector2<f64> {
        let (x, y) = (n.x, n.y);
        let r2 = x * x + y * y;
        let radial = 1.0 + self.k1 * r2 + self.k2 * r2 * r2 + self.k3 * r2 * r2 * r2;
        Vector2::new(
            x * radial + 2.0 * self.p1 * x * y + self.p2 * (r2 + 2.0 * x * x),
            y * radial + self.p1 * (r2 + 2.0 * y * y) + 2.0 * self.p2 * x * y,
        )
    }

    /// Invert [`Distortion::distort`] by fixed-point iteration. Converges for the moderate
    /// distortion of real lenses inside the field of view; otherwise returns `NoConvergence`.
    pub fn undistort(&self, distorted: Vector2<f64>) -> Result<Vector2<f64>, GeomError> {
        let mut n = distorted;
        for _ in 0..Self::MAX_ITERATIONS {
            let error = self.distort(n) - distorted;
            if !error.x.is_finite() || !error.y.is_finite() {
                return Err(GeomError::NoConvergence);
            }
            n -= error;
            if error.norm() < Self::TOLERANCE {
                return Ok(n);
            }
        }
        Err(GeomError::NoConvergence)
    }
}

/// Pinhole camera with lens distortion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Ideal pinhole parameters.
    pub intrinsics: Pinhole,
    /// Lens distortion applied on top of the pinhole model.
    pub distortion: Distortion,
}

impl Camera {
    /// Project a camera-frame 3D point to distorted pixel coordinates.
    pub fn project(&self, p: &Point3<f64>) -> Result<Point2<f64>, GeomError> {
        if p.z <= 0.0 || !p.z.is_finite() {
            return Err(GeomError::BehindCamera);
        }
        let d = self.distortion.distort(Vector2::new(p.x / p.z, p.y / p.z));
        Ok(Point2::new(
            self.intrinsics.fx * d.x + self.intrinsics.cx,
            self.intrinsics.fy * d.y + self.intrinsics.cy,
        ))
    }

    /// Back-project a distorted pixel at a known depth to a camera-frame 3D point.
    pub fn unproject(&self, px: &Point2<f64>, depth: f64) -> Result<Point3<f64>, GeomError> {
        if depth <= 0.0 || !depth.is_finite() {
            return Err(GeomError::InvalidDepth);
        }
        let d = Vector2::new(
            (px.x - self.intrinsics.cx) / self.intrinsics.fx,
            (px.y - self.intrinsics.cy) / self.intrinsics.fy,
        );
        let n = self.distortion.undistort(d)?;
        Ok(Point3::new(n.x * depth, n.y * depth, depth))
    }

    /// Root-mean-square reprojection error in pixels of world points seen at `pixels`,
    /// given the world-to-camera transform.
    pub fn reprojection_rms(
        &self,
        world_to_camera: &Isometry3<f64>,
        world: &[Point3<f64>],
        pixels: &[Point2<f64>],
    ) -> Result<f64, GeomError> {
        if world.len() != pixels.len() {
            return Err(GeomError::LengthMismatch);
        }
        if world.is_empty() {
            return Err(GeomError::EmptyInput);
        }
        let mut sum = 0.0;
        for (w, observed) in world.iter().zip(pixels) {
            let projected = self.project(&(world_to_camera * w))?;
            sum += (projected - observed).norm_squared();
        }
        Ok((sum / world.len() as f64).sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::{Translation3, UnitQuaternion};
    use proptest::prelude::*;

    const PINHOLE: Pinhole = Pinhole {
        fx: 900.0,
        fy: 905.0,
        cx: 640.0,
        cy: 360.0,
    };
    const LENS: Distortion = Distortion {
        k1: -0.12,
        k2: 0.03,
        p1: 0.001,
        p2: -0.0005,
        k3: 0.0,
    };
    const CAM: Camera = Camera {
        intrinsics: PINHOLE,
        distortion: LENS,
    };

    #[test]
    fn behind_camera_is_rejected() {
        assert_eq!(
            PINHOLE.project(&Point3::new(0.0, 0.0, -1.0)),
            Err(GeomError::BehindCamera)
        );
        assert_eq!(
            CAM.project(&Point3::new(0.0, 0.0, 0.0)),
            Err(GeomError::BehindCamera)
        );
    }

    #[test]
    fn bad_depth_is_rejected() {
        assert_eq!(
            PINHOLE.unproject(&Point2::new(0.0, 0.0), 0.0),
            Err(GeomError::InvalidDepth)
        );
        assert_eq!(
            CAM.unproject(&Point2::new(0.0, 0.0), f64::NAN),
            Err(GeomError::InvalidDepth)
        );
    }

    #[test]
    fn zero_distortion_matches_pinhole() -> Result<(), GeomError> {
        let cam = Camera {
            intrinsics: PINHOLE,
            distortion: Distortion::default(),
        };
        let p = Point3::new(0.2, -0.1, 1.5);
        assert!((cam.project(&p)? - PINHOLE.project(&p)?).norm() < 1e-12);
        Ok(())
    }

    #[test]
    fn distortion_moves_off_axis_points() -> Result<(), GeomError> {
        let p = Point3::new(0.5, 0.3, 1.0);
        let moved = (CAM.project(&p)? - PINHOLE.project(&p)?).norm();
        assert!(moved > 1.0, "expected visible distortion, got {moved} px");
        Ok(())
    }

    #[test]
    fn absurd_distortion_reports_no_convergence() {
        let bad = Distortion {
            k1: 50.0,
            ..Distortion::default()
        };
        assert_eq!(
            bad.undistort(Vector2::new(2.0, 2.0)),
            Err(GeomError::NoConvergence)
        );
    }

    #[test]
    fn reprojection_rejects_bad_input() {
        let pose = Isometry3::identity();
        assert_eq!(
            CAM.reprojection_rms(&pose, &[], &[]),
            Err(GeomError::EmptyInput)
        );
        assert_eq!(
            CAM.reprojection_rms(&pose, &[Point3::new(0.0, 0.0, 1.0)], &[]),
            Err(GeomError::LengthMismatch)
        );
    }

    #[test]
    fn reprojection_of_known_offset_is_exact() -> Result<(), GeomError> {
        let pose = Isometry3::from_parts(
            Translation3::new(0.1, -0.05, 2.0),
            UnitQuaternion::from_euler_angles(0.1, -0.2, 0.3),
        );
        let world: Vec<Point3<f64>> = [
            (0.0, 0.0, 0.0),
            (0.1, 0.0, 0.05),
            (-0.1, 0.1, 0.0),
            (0.0, -0.1, 0.1),
        ]
        .iter()
        .map(|&(x, y, z)| Point3::new(x, y, z))
        .collect();
        let mut pixels = Vec::new();
        for w in &world {
            pixels.push(CAM.project(&(pose * w))?);
        }
        assert!(CAM.reprojection_rms(&pose, &world, &pixels)? < 1e-9);
        let shifted: Vec<Point2<f64>> =
            pixels.iter().map(|p| Point2::new(p.x + 1.0, p.y)).collect();
        assert!((CAM.reprojection_rms(&pose, &world, &shifted)? - 1.0).abs() < 1e-9);
        Ok(())
    }

    proptest! {
        #[test]
        fn pinhole_round_trips(x in -1.0f64..1.0, y in -1.0f64..1.0, z in 0.2f64..3.0) {
            let p = Point3::new(x, y, z);
            let px = PINHOLE.project(&p).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let q = PINHOLE.unproject(&px, z).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!((p - q).norm() < 1e-9);
        }

        #[test]
        fn distorted_camera_round_trips(
            nx in -0.5f64..0.5, ny in -0.5f64..0.5, z in 0.3f64..3.0,
            k1 in -0.2f64..0.2, k2 in -0.05f64..0.05,
            p1 in -0.005f64..0.005, p2 in -0.005f64..0.005,
        ) {
            let cam = Camera {
                intrinsics: PINHOLE,
                distortion: Distortion { k1, k2, p1, p2, k3: 0.0 },
            };
            let p = Point3::new(nx * z, ny * z, z);
            let px = cam.project(&p).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let q = cam.unproject(&px, z).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert!((p - q).norm() < 1e-8, "round trip error {}", (p - q).norm());
        }
    }
}
