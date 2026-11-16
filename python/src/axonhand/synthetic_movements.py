"""Synthetic opening and closing, turning and tremor with known truth.

The same idea as :mod:`axonhand.synthetic`: the generator knows exactly what it put in, so an
analysis is checked against the truth. Distances are in palm lengths; the wrist is at the origin
and the middle finger base one palm above it. Where the three movements share a hand, the
landmarks they do not move stay where the template puts them.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import (
    FINGERTIPS,
    INDEX_MCP,
    LANDMARK_COUNT,
    MIDDLE_MCP,
    PINKY_MCP,
    WRIST,
    LandmarkTrack,
)

_RING_MCP = 13
_TIP_ANGLES_DEG = (-45.0, -15.0, 0.0, 12.0, 25.0)
"""Direction of the thumb to little fingertip from straight up, when the hand is open."""
_PALM_HALF_WIDTH = 0.35


def _hand() -> NDArray[np.float64]:
    """A flat open hand facing the camera, as a ``(21, 3)`` array in palm lengths."""
    points = np.zeros((LANDMARK_COUNT, 3))
    for landmark, x in ((INDEX_MCP, -0.35), (MIDDLE_MCP, 0.0), (_RING_MCP, 0.2), (PINKY_MCP, 0.35)):
        points[landmark] = (x, 1.0, 0.0)
    points[WRIST] = (0.0, 0.0, 0.0)
    for tip, angle in zip(FINGERTIPS, _TIP_ANGLES_DEG, strict=True):
        a = np.deg2rad(angle)
        points[tip] = (np.sin(a) * 1.9, np.cos(a) * 1.9, 0.0)
    return points


def _fill(frames: int) -> NDArray[np.float64]:
    return np.tile(_hand(), (frames, 1, 1))


@dataclass(frozen=True)
class OpenCloseSpec:
    """What to generate for opening and closing the hand.

    Attributes:
        rate_hz: cycles per second.
        cycles: number of openings.
        fps: frames per second.
        open_distance: fingertip distance from the wrist when open, in palm lengths.
        closed_distance: the same in a fist.
        decrement: fraction by which the range falls linearly from the first to the last cycle.
        noise: standard deviation of position noise, in palm lengths.
        lead_in: seconds with the hand closed before the first cycle.
        seed: random seed for the noise.
    """

    rate_hz: float = 2.0
    cycles: int = 20
    fps: float = 60.0
    open_distance: float = 1.9
    closed_distance: float = 1.0
    decrement: float = 0.0
    noise: float = 0.0
    lead_in: float = 0.5
    seed: int = 0


@dataclass(frozen=True)
class OpenCloseTruth:
    """What the generator put in."""

    peak_times: NDArray[np.float64]
    ranges: NDArray[np.float64]


def generate_open_close(spec: OpenCloseSpec) -> tuple[LandmarkTrack, OpenCloseTruth]:
    """A landmark track of opening and closing the hand, and its truth."""
    period = 1.0 / spec.rate_hz
    starts = spec.lead_in + period * np.arange(spec.cycles)
    ranges = (spec.open_distance - spec.closed_distance) * (
        1.0 - spec.decrement * np.arange(spec.cycles) / (spec.cycles - 1)
    )
    times = np.arange(int(round((starts[-1] + period + 0.5) * spec.fps))) / spec.fps
    reach = np.full(times.size, spec.closed_distance)
    for k in range(spec.cycles):
        inside = (times >= starts[k]) & (times < starts[k] + period)
        phase = (times[inside] - starts[k]) / period
        reach[inside] = spec.closed_distance + ranges[k] * np.sin(np.pi * phase) ** 2
    points = _fill(times.size)
    for tip in FINGERTIPS:
        points[:, tip, :] = (
            points[:, WRIST, :] + (_hand()[tip] / np.linalg.norm(_hand()[tip])) * reach[:, None]
        )
    if spec.noise > 0:
        rng = np.random.default_rng(spec.seed)
        points[:, :, :2] += rng.normal(0.0, spec.noise, size=points[:, :, :2].shape)
    return (
        LandmarkTrack(fps=spec.fps, points=points),
        OpenCloseTruth(
            peak_times=np.asarray(starts + period / 2, dtype=np.float64),
            ranges=np.asarray(ranges, dtype=np.float64),
        ),
    )


@dataclass(frozen=True)
class TurnSpec:
    """What to generate for turning the hand over and back.

    Attributes:
        rate_hz: full cycles (palm to the camera, away, and back) per second.
        cycles: number of full cycles.
        fps: frames per second.
        swing_degrees: how far the palm turns each way from edge-on; 90 shows each face fully.
        decrement: fraction by which the swing falls linearly from the first to the last cycle.
        noise: standard deviation of position noise, in palm lengths.
        lead_in: seconds with the hand edge on before the turning starts.
        seed: random seed for the noise.
    """

    rate_hz: float = 1.5
    cycles: int = 12
    fps: float = 60.0
    swing_degrees: float = 90.0
    decrement: float = 0.0
    noise: float = 0.0
    lead_in: float = 0.0
    seed: int = 0


@dataclass(frozen=True)
class TurnTruth:
    """What the generator put in."""

    turn_times: NDArray[np.float64]
    """Time at which the hand is edge on, for each turn."""


def generate_turns(spec: TurnSpec) -> tuple[LandmarkTrack, TurnTruth]:
    """A landmark track of the palm turning to and away from the camera, and its truth."""
    duration = spec.lead_in + spec.cycles / spec.rate_hz
    times = np.arange(int(round(duration * spec.fps))) / spec.fps
    local = np.clip(times - spec.lead_in, 0.0, None)
    swing = np.deg2rad(spec.swing_degrees) * (
        1.0 - spec.decrement * np.clip(local * spec.rate_hz / spec.cycles, 0.0, 1.0)
    )
    # Angle of the palm normal from the camera axis: face-on at 0, edge-on at 90, back at 180.
    angle = np.pi / 2 + swing * np.sin(2 * np.pi * spec.rate_hz * local)
    points = _fill(times.size)
    for landmark in (INDEX_MCP, _RING_MCP, PINKY_MCP):
        points[:, landmark, 0] = _hand()[landmark, 0] * np.cos(angle)
        points[:, landmark, 2] = _hand()[landmark, 0] * np.sin(angle)
    for tip, angle_deg in zip(FINGERTIPS, _TIP_ANGLES_DEG, strict=True):
        points[:, tip, 0] = np.sin(np.deg2rad(angle_deg)) * 1.9 * np.cos(angle)
    if spec.noise > 0:
        rng = np.random.default_rng(spec.seed)
        points[:, :, :2] += rng.normal(0.0, spec.noise, size=points[:, :, :2].shape)
    # Edge on at every half cycle after the start (the first at local time 0 is not a turn).
    count = 2 * spec.cycles
    turn_times = spec.lead_in + np.arange(1, count) / (2 * spec.rate_hz)
    return LandmarkTrack(fps=spec.fps, points=points), TurnTruth(turn_times=turn_times)


@dataclass(frozen=True)
class TremorSpec:
    """What to generate for a postural tremor.

    Attributes:
        frequency_hz: tremor frequency.
        rms: root mean square of the tremor along its direction, in palm lengths.
        fps: frames per second.
        seconds: length of the video.
        drift: peak size of a slow (0.2 Hz) drift of the whole hand, in palm lengths.
        noise: standard deviation of tracker noise on each coordinate, in palm lengths.
        seed: random seed for the noise.
    """

    frequency_hz: float = 5.0
    rms: float = 0.02
    fps: float = 60.0
    seconds: float = 15.0
    drift: float = 0.05
    noise: float = 0.0005
    seed: int = 0


def generate_tremor(spec: TremorSpec) -> LandmarkTrack:
    """A landmark track of a hand held out with a tremor, a slow drift and tracker noise."""
    times = np.arange(int(round(spec.seconds * spec.fps))) / spec.fps
    rng = np.random.default_rng(spec.seed)
    amplitude = spec.rms * np.sqrt(2.0)
    tremor = amplitude * np.sin(2 * np.pi * spec.frequency_hz * times + rng.uniform(0, 2 * np.pi))
    drift = spec.drift * np.sin(2 * np.pi * 0.2 * times)
    points = _fill(times.size)
    direction = np.array([0.8, 0.6])
    points[:, :, :2] += (tremor + drift)[:, None, None] * direction
    points[:, :, :2] += rng.normal(0.0, spec.noise, size=points[:, :, :2].shape)
    return LandmarkTrack(fps=spec.fps, points=points)
