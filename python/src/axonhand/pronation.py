"""Turning the hand over and back (MDS-UPDRS 3.6): from a landmark track to the measures.

With the forearm upright and the palm turning towards and away from the camera, the triangle
wrist - index knuckle - little-finger knuckle changes between the two faces of the hand: its
signed area (in palm lengths squared) is positive with one face to the camera, negative with the
other, and zero when the hand is edge on. A *turn* is one crossing of zero, found with a dead
band so that jitter near zero is not counted twice. A full pronation and supination is two turns.

Only the number, timing and relative size of the turns are measured. The angle turned through is
not, because one camera cannot tell how far a hand that never reaches a face has turned.

Everything here is research-grade measurement, not a clinical score; see ``docs/METRICS.md``.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import INDEX_MCP, PINKY_MCP, WRIST, LandmarkTrack
from axonhand.tapping import AnalysisError, TappingParameters, gap_counts, palm_length, smooth

MIN_TURNS = 6
"""Fewest turns (three full cycles) that give a rate and a slowing."""
DEAD_BAND = 0.25
"""A turn is counted when the signal passes from beyond this fraction of the reference to beyond
the same fraction on the other side."""
_MIN_REFERENCE = 0.02
"""Smallest reference area, in palm lengths squared; below it the faces cannot be told apart."""


@dataclass(frozen=True)
class TurnResult:
    """Everything measured from one track."""

    turn_times: NDArray[np.float64]
    """Time at which the hand was edge on, for each turn, in seconds."""
    excursions: NDArray[np.float64]
    """For each complete half-turn between two turns, the largest face-on value reached, as a
    fraction of the largest over the whole video (1.0 is the most turned-over hand seen)."""
    turn_count: int
    cycle_rate_hz: float
    """Full cycles (two turns) per second between the first and last turn."""
    excursion_variability_percent: float
    excursion_decrement_percent: float
    """Fall of the fitted excursion from the first to the last half-turn; positive means it fell."""
    hesitations: int
    arrests: int
    missing_fraction: float


def face_signal(track: LandmarkTrack) -> tuple[NDArray[np.float64], float]:
    """Signed area of the wrist - index knuckle - little knuckle triangle, in palm lengths squared.

    Raises:
        AnalysisError: if the hand size is unknown or more than half the frames are missing.
    """
    palm = palm_length(track)
    a = track.points[:, INDEX_MCP, :2] - track.points[:, WRIST, :2]
    b = track.points[:, PINKY_MCP, :2] - track.points[:, WRIST, :2]
    raw = (a[:, 0] * b[:, 1] - a[:, 1] * b[:, 0]) / palm**2
    missing = ~np.isfinite(raw)
    fraction = float(missing.mean())
    if fraction > 0.5:
        raise AnalysisError("The knuckles were not found in more than half of the video.")
    if missing.any():
        index = np.arange(raw.size)
        raw = np.interp(index, index[~missing], raw[~missing])
    return raw, fraction


def _turns(signal: NDArray[np.float64]) -> list[float]:
    """Fractional frame positions of the turns."""
    state = 0
    last_high = -1  # last frame at which the signal was beyond the dead band on the current side
    crossings: list[float] = []
    for index, value in enumerate(signal):
        side = 1 if value > DEAD_BAND else -1 if value < -DEAD_BAND else 0
        if side == 0:
            continue
        if state != 0 and side != state:
            # Between the last frame on the old side and this one, find where it passes zero.
            seg = signal[last_high : index + 1]
            sign_change = np.nonzero(np.sign(seg[:-1]) != np.sign(seg[1:]))[0]
            if sign_change.size:
                at = last_high + int(sign_change[0])
                a, b = float(signal[at]), float(signal[at + 1])
                crossings.append(at + (a / (a - b) if a != b else 0.5))
            else:
                crossings.append(float(index))
        state = side
        last_high = index
    return crossings


def analyse_turns(track: LandmarkTrack, parameters: TappingParameters | None = None) -> TurnResult:
    """Measure turning of the hand on ``track``.

    Only the hesitation and arrest factors of ``parameters`` are used.

    Raises:
        AnalysisError: if the faces of the hand cannot be told apart, too much is missing, or
            fewer than ``MIN_TURNS`` turns are found.
    """
    params = parameters or TappingParameters()
    raw, missing = face_signal(track)
    reference = float(np.percentile(np.abs(raw), 95))
    if reference < _MIN_REFERENCE:
        raise AnalysisError(
            "The front and back of the hand could not be told apart. Film with the forearm "
            "upright and the palm turning towards and away from the camera."
        )
    signal = smooth(raw) / reference
    crossings = _turns(signal)
    if len(crossings) < MIN_TURNS:
        raise AnalysisError(
            f"Only {len(crossings)} turns of the hand were found; at least {MIN_TURNS} are needed."
        )
    times = np.asarray(crossings) / track.fps
    # Each half-turn lies between two consecutive turns: its extreme is the face-on value.
    peaks = []
    for start, stop in zip(crossings[:-1], crossings[1:], strict=True):
        seg = np.abs(signal[int(np.ceil(start)) : int(np.floor(stop)) + 1])
        peaks.append(float(seg.max()) if seg.size else 0.0)
    excursions = np.asarray(peaks) / max(float(np.max(peaks)), 1e-9)
    hesitations, arrests = gap_counts(times, params)
    order = np.arange(excursions.size, dtype=np.float64)
    slope, intercept = np.polyfit(order, excursions, 1)
    last_fit = intercept + slope * order[-1]
    decrement = 100.0 * (intercept - last_fit) / intercept if intercept > 0 else 0.0
    mean = float(excursions.mean())
    return TurnResult(
        turn_times=times,
        excursions=excursions,
        turn_count=len(crossings),
        cycle_rate_hz=float(0.5 * (len(crossings) - 1) / (times[-1] - times[0])),
        excursion_variability_percent=float(100.0 * excursions.std() / mean) if mean > 0 else 0.0,
        excursion_decrement_percent=float(decrement),
        hesitations=hesitations,
        arrests=arrests,
        missing_fraction=missing,
    )
