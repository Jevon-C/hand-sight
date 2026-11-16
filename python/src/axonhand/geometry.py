"""Geometry functions backed by the Rust core (crates/geom).

This module is the public Python face of the compiled extension. All functions raise
``ValueError`` with a plain message when given input that cannot be solved, for example
too few points or points on a line.
"""

from axonhand._geom import (
    Camera,
    Pose,
    fit_plane_ransac,
    pose_from_planar_points,
    rigid_align,
    smooth_track,
)

__all__ = [
    "Camera",
    "Pose",
    "fit_plane_ransac",
    "pose_from_planar_points",
    "rigid_align",
    "smooth_track",
]
