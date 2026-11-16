"""Tests for the Rust geometry core as seen from Python.

The reprojection ceiling test is the contract: the pose recovered from a view of a known
board must reproject onto the observed pixels to within a fixed, small number of pixels.
"""

import math

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from axonhand.geometry import (
    Camera,
    Pose,
    fit_plane_ransac,
    pose_from_planar_points,
    rigid_align,
    smooth_track,
)

# Noise-free views must reproject to well under a thousandth of a pixel.
REPROJECTION_CEILING_PX = 1e-3
# With about half a pixel of jitter, the fit may not be worse than the jitter itself.
NOISY_REPROJECTION_CEILING_PX = 0.8

CAMERA = Camera(900.0, 905.0, 640.0, 360.0, k1=-0.12, k2=0.03, p1=0.001, p2=-0.0005)
BOARD = [(i * 0.03 - 0.075, j * 0.03 - 0.045) for i in range(6) for j in range(4)]
WORLD = [(x, y, 0.0) for x, y in BOARD]


def rotation_xyz(rx: float, ry: float, rz: float) -> list[list[float]]:
    """Rotation matrix for angles about x, y, z (applied in that order)."""
    cx, sx = math.cos(rx), math.sin(rx)
    cy, sy = math.cos(ry), math.sin(ry)
    cz, sz = math.cos(rz), math.sin(rz)
    rot_x = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]]
    rot_y = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]]
    rot_z = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]]

    def mul(a: list[list[float]], b: list[list[float]]) -> list[list[float]]:
        return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]

    return mul(rot_z, mul(rot_y, rot_x))


def view(pose: Pose) -> list[list[float]]:
    return [CAMERA.project(pose.apply(p)) for p in WORLD]


def test_pose_reprojects_within_ceiling() -> None:
    truth = Pose(rotation_xyz(0.4, -0.3, 0.2), [0.02, -0.01, 0.45])
    pixels = view(truth)
    found = pose_from_planar_points(CAMERA, BOARD, pixels)
    assert CAMERA.reprojection_rms(found, WORLD, pixels) < REPROJECTION_CEILING_PX


def test_pose_with_noise_stays_under_noisy_ceiling() -> None:
    truth = Pose(rotation_xyz(0.2, 0.3, -0.1), [0.0, 0.0, 0.5])
    pixels = [
        [u + 0.5 * math.sin(k * 2.4), v + 0.5 * math.cos(k * 1.2)]
        for k, (u, v) in enumerate(view(truth))
    ]
    found = pose_from_planar_points(CAMERA, BOARD, pixels)
    assert CAMERA.reprojection_rms(found, WORLD, pixels) < NOISY_REPROJECTION_CEILING_PX


@settings(max_examples=40, deadline=None)
@given(
    rx=st.floats(-0.6, 0.6),
    ry=st.floats(-0.6, 0.6),
    rz=st.floats(-3.0, 3.0),
    tx=st.floats(-0.05, 0.05),
    ty=st.floats(-0.05, 0.05),
    tz=st.floats(0.3, 0.8),
)
def test_pose_ceiling_holds_for_many_views(
    rx: float, ry: float, rz: float, tx: float, ty: float, tz: float
) -> None:
    truth = Pose(rotation_xyz(rx, ry, rz), [tx, ty, tz])
    pixels = view(truth)
    found = pose_from_planar_points(CAMERA, BOARD, pixels)
    assert CAMERA.reprojection_rms(found, WORLD, pixels) < REPROJECTION_CEILING_PX


def test_rigid_align_recovers_transform() -> None:
    truth = Pose(rotation_xyz(0.3, -0.5, 1.1), [0.5, -1.0, 2.0])
    source = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.3, 0.4, 1.5], [-0.7, 0.9, 0.2]]
    target = [truth.apply(p) for p in source]
    found = rigid_align(source, target)
    for p, t in zip(source, target, strict=True):
        assert math.dist(found.apply(p), t) < 1e-9


def test_plane_fit_ignores_outliers() -> None:
    plane = [[i * 0.1, j * 0.1, 0.5 * i * 0.1 + 2.0] for i in range(8) for j in range(8)]
    outliers = [[0.1 * k, 0.7, 5.0 + k] for k in range(10)]
    normal, offset, inliers = fit_plane_ransac(plane + outliers, 1e-6, 200, 7)
    assert inliers == list(range(len(plane)))
    assert math.isclose(math.hypot(*normal), 1.0)
    for p in plane:
        assert abs(sum(n * c for n, c in zip(normal, p, strict=True)) - offset) < 1e-9


def test_smooth_track_reduces_noise() -> None:
    truth = [math.sin(k / 30.0) for k in range(300)]
    noisy = [t + 0.14 * math.sin(k * 12.9898) for k, t in enumerate(truth)]
    smooth = smooth_track(noisy, 1 / 60, 50.0, 0.01)

    def rms(a: list[float], b: list[float]) -> float:
        return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b, strict=True)) / len(a))

    assert rms(smooth, truth) < 0.6 * rms(noisy, truth)


def test_errors_become_value_errors() -> None:
    with pytest.raises(ValueError, match="too few points"):
        rigid_align([[0, 0, 0]], [[0, 0, 0]])
    with pytest.raises(ValueError, match="out of range"):
        smooth_track([1.0, 2.0], 0.0, 1.0, 1.0)
    with pytest.raises(ValueError, match="behind the camera"):
        CAMERA.project([0.0, 0.0, -1.0])
