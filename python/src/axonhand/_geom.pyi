"""Type stubs for the compiled geometry extension (built from crates/geom-py)."""

from collections.abc import Sequence

Point2 = Sequence[float]
Point3 = Sequence[float]

class Pose:
    """A rigid transform: a 3x3 rotation (row-major) followed by a translation."""

    rotation: list[list[float]]
    translation: list[float]
    def __init__(
        self, rotation: Sequence[Sequence[float]], translation: Sequence[float]
    ) -> None: ...
    def apply(self, point: Point3) -> list[float]: ...

class Camera:
    """Pinhole camera with Brown-Conrady lens distortion."""

    def __init__(
        self,
        fx: float,
        fy: float,
        cx: float,
        cy: float,
        k1: float = 0.0,
        k2: float = 0.0,
        p1: float = 0.0,
        p2: float = 0.0,
        k3: float = 0.0,
    ) -> None: ...
    def project(self, point: Point3) -> list[float]: ...
    def reprojection_rms(
        self,
        world_to_camera: Pose,
        world: Sequence[Point3],
        pixels: Sequence[Point2],
    ) -> float: ...

def rigid_align(source: Sequence[Point3], target: Sequence[Point3]) -> Pose: ...
def fit_plane_ransac(
    points: Sequence[Point3], threshold: float, iterations: int, seed: int
) -> tuple[list[float], float, list[int]]: ...
def pose_from_planar_points(
    camera: Camera, board: Sequence[Point2], pixels: Sequence[Point2]
) -> Pose: ...
def smooth_track(
    values: Sequence[float],
    dt: float,
    acceleration_variance: float,
    measurement_variance: float,
) -> list[float]: ...
