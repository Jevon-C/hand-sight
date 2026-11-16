//! Python bindings for the Hand Sight geometry core.
//!
//! The functions here only convert between Python values and the Rust types, then call
//! `axonhand-geom`. All maths lives in that crate. Rust errors become Python `ValueError`.

use axonhand_geom::{
    fit_plane_ransac, pose_from_planar_points, rigid_align, smooth_track, Camera, Distortion,
    GeomError, Pinhole, SmootherConfig,
};
use nalgebra::{Isometry3, Point2, Point3};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn to_py(error: GeomError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn points3(items: &[[f64; 3]]) -> Vec<Point3<f64>> {
    items
        .iter()
        .map(|p| Point3::new(p[0], p[1], p[2]))
        .collect()
}

fn points2(items: &[[f64; 2]]) -> Vec<Point2<f64>> {
    items.iter().map(|p| Point2::new(p[0], p[1])).collect()
}

/// A rigid transform: a 3x3 rotation (row-major) followed by a translation.
#[pyclass(frozen, module = "axonhand._geom")]
#[derive(Clone, Debug)]
struct Pose {
    /// Rotation matrix, three rows of three numbers.
    #[pyo3(get)]
    rotation: [[f64; 3]; 3],
    /// Translation vector.
    #[pyo3(get)]
    translation: [f64; 3],
}

impl Pose {
    fn from_isometry(iso: &Isometry3<f64>) -> Self {
        let m = iso.rotation.to_rotation_matrix();
        let m = m.matrix();
        let rotation = std::array::from_fn(|r| std::array::from_fn(|c| m[(r, c)]));
        Self {
            rotation,
            translation: [iso.translation.x, iso.translation.y, iso.translation.z],
        }
    }

    fn to_isometry(&self) -> Isometry3<f64> {
        let r = self.rotation;
        let matrix = nalgebra::Matrix3::new(
            r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2],
        );
        let rotation = nalgebra::UnitQuaternion::from_rotation_matrix(
            &nalgebra::Rotation3::from_matrix_unchecked(matrix),
        );
        let t = self.translation;
        Isometry3::from_parts(nalgebra::Translation3::new(t[0], t[1], t[2]), rotation)
    }
}

#[pymethods]
impl Pose {
    #[new]
    fn new(rotation: [[f64; 3]; 3], translation: [f64; 3]) -> Self {
        Self {
            rotation,
            translation,
        }
    }

    /// Apply the transform to a 3D point.
    fn apply(&self, point: [f64; 3]) -> [f64; 3] {
        let p = self.to_isometry() * Point3::new(point[0], point[1], point[2]);
        [p.x, p.y, p.z]
    }
}

/// A pinhole camera with Brown-Conrady lens distortion.
#[pyclass(frozen, name = "Camera", module = "axonhand._geom")]
#[derive(Clone, Debug)]
struct PyCamera {
    inner: Camera,
}

#[pymethods]
impl PyCamera {
    #[new]
    #[pyo3(signature = (fx, fy, cx, cy, k1=0.0, k2=0.0, p1=0.0, p2=0.0, k3=0.0))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        fx: f64,
        fy: f64,
        cx: f64,
        cy: f64,
        k1: f64,
        k2: f64,
        p1: f64,
        p2: f64,
        k3: f64,
    ) -> Self {
        Self {
            inner: Camera {
                intrinsics: Pinhole { fx, fy, cx, cy },
                distortion: Distortion { k1, k2, p1, p2, k3 },
            },
        }
    }

    /// Project a camera-frame point to distorted pixel coordinates.
    fn project(&self, point: [f64; 3]) -> PyResult<[f64; 2]> {
        let p = self
            .inner
            .project(&Point3::new(point[0], point[1], point[2]))
            .map_err(to_py)?;
        Ok([p.x, p.y])
    }

    /// Root-mean-square reprojection error in pixels, for world points seen at `pixels`.
    /// `world_to_camera` maps world coordinates into the camera frame.
    fn reprojection_rms(
        &self,
        world_to_camera: &Pose,
        world: Vec<[f64; 3]>,
        pixels: Vec<[f64; 2]>,
    ) -> PyResult<f64> {
        self.inner
            .reprojection_rms(
                &world_to_camera.to_isometry(),
                &points3(&world),
                &points2(&pixels),
            )
            .map_err(to_py)
    }
}

/// Best rigid transform taking `source` onto `target`.
#[pyfunction]
#[pyo3(name = "rigid_align")]
fn py_rigid_align(source: Vec<[f64; 3]>, target: Vec<[f64; 3]>) -> PyResult<Pose> {
    let iso = rigid_align(&points3(&source), &points3(&target)).map_err(to_py)?;
    Ok(Pose::from_isometry(&iso))
}

/// Robust plane fit. Returns the unit normal, the offset, and the inlier indices.
#[pyfunction]
#[pyo3(name = "fit_plane_ransac")]
fn py_fit_plane_ransac(
    points: Vec<[f64; 3]>,
    threshold: f64,
    iterations: usize,
    seed: u64,
) -> PyResult<([f64; 3], f64, Vec<usize>)> {
    let fit = fit_plane_ransac(&points3(&points), threshold, iterations, seed).map_err(to_py)?;
    let n = fit.plane.normal;
    Ok(([n.x, n.y, n.z], fit.plane.offset, fit.inliers))
}

/// Pose of a flat target (the "camera from board" transform).
#[pyfunction]
#[pyo3(name = "pose_from_planar_points")]
fn py_pose_from_planar_points(
    camera: &PyCamera,
    board: Vec<[f64; 2]>,
    pixels: Vec<[f64; 2]>,
) -> PyResult<Pose> {
    let iso = pose_from_planar_points(&camera.inner, &points2(&board), &points2(&pixels))
        .map_err(to_py)?;
    Ok(Pose::from_isometry(&iso))
}

/// Smooth a scalar track; NaN entries are treated as missing.
#[pyfunction]
#[pyo3(name = "smooth_track")]
fn py_smooth_track(
    values: Vec<f64>,
    dt: f64,
    acceleration_variance: f64,
    measurement_variance: f64,
) -> PyResult<Vec<f64>> {
    let config = SmootherConfig {
        dt,
        acceleration_variance,
        measurement_variance,
    };
    smooth_track(&values, &config).map_err(to_py)
}

#[pymodule]
fn _geom(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Pose>()?;
    module.add_class::<PyCamera>()?;
    module.add_function(wrap_pyfunction!(py_rigid_align, module)?)?;
    module.add_function(wrap_pyfunction!(py_fit_plane_ransac, module)?)?;
    module.add_function(wrap_pyfunction!(py_pose_from_planar_points, module)?)?;
    module.add_function(wrap_pyfunction!(py_smooth_track, module)?)?;
    Ok(())
}
