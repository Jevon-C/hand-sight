//! Geometry core for Hand Sight.
//!
//! Camera models with lens distortion, rigid alignment, plane fitting, pose from a planar
//! target, and smoothing of noisy tracks. Everything is pure Rust with no unsafe code and
//! returns [`GeomError`] instead of panicking.

#![deny(missing_docs)]

mod align;
mod camera;
mod error;
mod plane;
mod pose;
mod smooth;

pub use align::{alignment_rms, rigid_align};
pub use camera::{Camera, Distortion, Pinhole};
pub use error::GeomError;
pub use plane::{fit_plane, fit_plane_ransac, Plane, PlaneFit};
pub use pose::pose_from_planar_points;
pub use smooth::{smooth_points, smooth_track, SmootherConfig};
