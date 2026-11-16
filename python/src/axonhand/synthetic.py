"""Synthetic finger tapping with known truth.

Used as a test fixture and as the stand-in for a real hand model during development. The
generator knows exactly when each tap happens and how large it is, so an analysis can be checked
against the truth instead of against another guess.

One *tap* is a cycle in which the thumb and index fingertips separate and come back together.
Within a cycle the tip distance is ``amplitude * sin^2(pi * phase)``, so the fingers touch at
the start and end of a cycle and are furthest apart in the middle. Distances are in palm
lengths.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import (
    INDEX_TIP,
    LANDMARK_COUNT,
    MIDDLE_MCP,
    THUMB_TIP,
    WRIST,
    LandmarkTrack,
)

# Marker colours used when a track is drawn as a video and found again by the stand-in model.
MARKER_COLOURS: dict[int, tuple[int, int, int]] = {
    WRIST: (0, 200, 0),
    MIDDLE_MCP: (230, 230, 0),
    THUMB_TIP: (230, 0, 0),
    INDEX_TIP: (0, 0, 230),
}


@dataclass(frozen=True)
class TapSpec:
    """What to generate.

    Attributes:
        rate_hz: taps per second while tapping.
        taps: number of taps (cycles).
        fps: frames per second of the track.
        amplitude: size of the first tap, in palm lengths.
        decrement: fraction by which the amplitude falls linearly from the first to the last tap
            (0.4 means the last tap is 40 percent smaller).
        pauses: ``(after_tap_index, extra_seconds)`` pairs; the fingers rest closed for that long
            after the given tap.
        noise: standard deviation of position noise, in palm lengths.
        lead_in: seconds of rest before the first tap.
        seed: random seed for the noise.
    """

    rate_hz: float = 3.0
    taps: int = 30
    fps: float = 60.0
    amplitude: float = 0.8
    decrement: float = 0.0
    pauses: Sequence[tuple[int, float]] = field(default_factory=tuple)
    noise: float = 0.0
    lead_in: float = 0.5
    seed: int = 0

    def __post_init__(self) -> None:
        if self.rate_hz <= 0 or self.taps < 2 or self.fps <= 0 or self.amplitude <= 0:
            raise ValueError("rate, taps, fps and amplitude must be positive (taps at least 2)")
        if not 0.0 <= self.decrement < 1.0:
            raise ValueError("decrement must be in [0, 1)")
        if self.noise < 0 or self.lead_in < 0:
            raise ValueError("noise and lead_in must not be negative")
        for after, extra in self.pauses:
            if not 0 <= after < self.taps - 1 or extra <= 0:
                raise ValueError("each pause needs 0 <= after_tap < taps-1 and extra > 0")


@dataclass(frozen=True)
class TapTruth:
    """What the generator put in, for checking an analysis."""

    peak_times: NDArray[np.float64]
    """Time of the widest opening of each tap, in seconds."""
    amplitudes: NDArray[np.float64]
    """Widest opening of each tap, in palm lengths."""
    intervals: NDArray[np.float64]
    """Seconds between consecutive peaks."""

    @property
    def tap_count(self) -> int:
        """Number of taps."""
        return int(self.peak_times.size)

    @property
    def rate_hz(self) -> float:
        """Taps per second, as (n-1) divided by the time from first to last peak."""
        return float((self.tap_count - 1) / (self.peak_times[-1] - self.peak_times[0]))


@dataclass(frozen=True)
class SyntheticTapping:
    """A generated track and its truth."""

    track: LandmarkTrack
    truth: TapTruth


def _template() -> NDArray[np.float64]:
    """A fixed hand shape in palm lengths: wrist at the origin, middle knuckle one palm up."""
    points = np.full((LANDMARK_COUNT, 3), np.nan)
    points[WRIST] = (0.0, 0.0, 0.0)
    points[MIDDLE_MCP] = (0.0, 1.0, 0.0)
    points[INDEX_TIP] = (0.5, 1.7, 0.0)
    return points


def generate_tapping(spec: TapSpec) -> SyntheticTapping:
    """Generate a landmark track of finger tapping and its truth."""
    period = 1.0 / spec.rate_hz
    pause_after = dict(spec.pauses)
    starts = np.empty(spec.taps)
    clock = spec.lead_in
    for k in range(spec.taps):
        starts[k] = clock
        clock += period + pause_after.get(k, 0.0)
    amplitudes = spec.amplitude * (1.0 - spec.decrement * np.arange(spec.taps) / (spec.taps - 1))
    total = starts[-1] + period + 0.5
    times = np.arange(int(round(total * spec.fps))) / spec.fps

    distance = np.zeros_like(times)
    for k in range(spec.taps):
        inside = (times >= starts[k]) & (times < starts[k] + period)
        phase = (times[inside] - starts[k]) / period
        distance[inside] = amplitudes[k] * np.sin(np.pi * phase) ** 2

    points = np.tile(_template(), (times.size, 1, 1))
    direction = np.array([-0.8, -0.6, 0.0])
    points[:, THUMB_TIP, :] = points[:, INDEX_TIP, :] + distance[:, None] * direction
    if spec.noise > 0:
        rng = np.random.default_rng(spec.seed)
        for landmark in (WRIST, MIDDLE_MCP, THUMB_TIP, INDEX_TIP):
            points[:, landmark, :2] += rng.normal(0.0, spec.noise, size=(times.size, 2))

    peaks = starts + period / 2
    truth = TapTruth(peak_times=peaks, amplitudes=amplitudes, intervals=np.diff(peaks))
    return SyntheticTapping(track=LandmarkTrack(fps=spec.fps, points=points), truth=truth)


def render_video(
    track: LandmarkTrack,
    path: Path,
    size: tuple[int, int] = (320, 240),
    radius: int = 5,
) -> None:
    """Draw the marker landmarks of ``track`` as coloured dots and encode an MP4.

    The result is a video a colour-tracking stand-in model can read, so the whole path from
    video file to measures can be exercised without a real hand model.
    """
    import av  # imported here so the geometry and analysis code does not need a video library

    width, height = size
    scale = 0.35 * height
    origin = np.array([width * 0.5, height * 0.9])
    with av.open(str(path), mode="w") as container:
        stream = container.add_stream("libx264", rate=int(round(track.fps)))
        stream.width, stream.height = width, height
        stream.pix_fmt = "yuv420p"
        stream.options = {"crf": "8", "preset": "veryfast"}
        for frame_points in track.points:
            image = np.full((height, width, 3), 200, dtype=np.uint8)
            for landmark in (WRIST, MIDDLE_MCP, THUMB_TIP, INDEX_TIP):
                x, y = frame_points[landmark, :2]
                if not (np.isfinite(x) and np.isfinite(y)):
                    continue
                cx, cy = origin[0] + scale * x, origin[1] - scale * y
                x0, x1 = max(int(cx) - radius - 1, 0), min(int(cx) + radius + 2, width)
                y0, y1 = max(int(cy) - radius - 1, 0), min(int(cy) + radius + 2, height)
                if x0 >= x1 or y0 >= y1:
                    continue
                yy, xx = np.mgrid[y0:y1, x0:x1]
                disc = (xx - cx) ** 2 + (yy - cy) ** 2 <= radius**2
                image[y0:y1, x0:x1][disc] = MARKER_COLOURS[landmark]
            frame = av.VideoFrame.from_ndarray(image, format="rgb24")
            for packet in stream.encode(frame):
                container.mux(packet)
        for packet in stream.encode():
            container.mux(packet)
