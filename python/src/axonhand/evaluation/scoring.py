"""Compare the taps the software found with the taps a person marked.

A tap is compared at the moment the fingertips touch (``tapping.contact_frames``), because that
moment is sharp in the video and in the signal, so two people agree on it closely.

Matching is one-to-one: each marked tap can be claimed by at most one found tap. A found tap
counts as a *hit* when it is within ``tolerance`` seconds of its partner; unmatched found taps
are *extra*, unmatched marked taps are *missed*. How exact the hits are is judged separately,
by the timing errors.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import LandmarkTrack
from axonhand.tapping import (
    AnalysisError,
    TappingParameters,
    TappingResult,
    analyse,
    gap_counts,
)

MIN_TOLERANCE_FRAMES = 2.0
TOLERANCE_FRACTION = 0.5
"""A tap is a hit when within this fraction of the typical gap between marked taps: the widest
tolerance that still leaves each tap exactly one possible partner."""


def tolerance_seconds(reference: NDArray[np.float64], fps: float) -> float:
    """How close in time a found tap must be to count as the marked one.

    Half the typical gap between marked taps, but never less than two frames, because
    a person cannot mark a moment more exactly than that.
    """
    floor = MIN_TOLERANCE_FRAMES / fps
    if reference.size < 2:
        return floor
    return max(floor, TOLERANCE_FRACTION * float(np.median(np.diff(reference))))


@dataclass(frozen=True)
class Matching:
    """Pairs of found and marked taps within tolerance."""

    found_index: tuple[int, ...]
    marked_index: tuple[int, ...]
    errors: NDArray[np.float64]
    """Found time minus marked time for each pair, in seconds; positive means found late."""
    extra: int
    missed: int


def match_taps(
    found: NDArray[np.float64], marked: NDArray[np.float64], tolerance: float
) -> Matching:
    """Pair found taps with marked taps, closest pairs first, each used at most once."""
    candidates = sorted(
        (abs(float(f) - float(m)), i, j)
        for i, f in enumerate(found)
        for j, m in enumerate(marked)
        if abs(float(f) - float(m)) <= tolerance
    )
    used_found: set[int] = set()
    used_marked: set[int] = set()
    pairs: list[tuple[int, int]] = []
    for _, i, j in candidates:
        if i not in used_found and j not in used_marked:
            used_found.add(i)
            used_marked.add(j)
            pairs.append((i, j))
    pairs.sort()
    errors = np.array([found[i] - marked[j] for i, j in pairs], dtype=np.float64)
    return Matching(
        found_index=tuple(i for i, _ in pairs),
        marked_index=tuple(j for _, j in pairs),
        errors=errors,
        extra=len(found) - len(pairs),
        missed=len(marked) - len(pairs),
    )


@dataclass(frozen=True)
class ClipScore:
    """How one clip compares. Rates are in taps per second."""

    video: str
    marked_count: int
    found_count: int
    hits: int
    extra: int
    missed: int
    timing_errors: tuple[float, ...]
    marked_rate: float
    found_rate: float
    marked_hesitations: int
    found_hesitations: int
    marked_arrests: int
    found_arrests: int
    tolerance: float
    missing_fraction: float
    missed_times: tuple[float, ...] = ()
    """Seconds of the marked taps that no found tap matched."""
    extra_times: tuple[float, ...] = ()
    """Seconds of the found taps that no marked tap matched."""

    @property
    def count_error(self) -> int:
        """Found minus marked."""
        return self.found_count - self.marked_count

    @property
    def rate_error_percent(self) -> float:
        """Found rate relative to the marked rate, as a signed percentage."""
        return 100.0 * (self.found_rate - self.marked_rate) / self.marked_rate


@dataclass(frozen=True)
class Failure:
    """A clip the software could not analyse. The reason counts against the software."""

    video: str
    reason: str


def _rate(times: NDArray[np.float64]) -> float:
    return float((times.size - 1) / (times[-1] - times[0]))


def score_clip(
    video: str,
    track: LandmarkTrack,
    marked: tuple[float, ...],
    parameters: TappingParameters | None = None,
) -> ClipScore | Failure:
    """Score one track against the taps marked on its video.

    Returns a :class:`Failure` if the software cannot analyse the track.

    Raises:
        ValueError: if fewer than four taps were marked (no rate or gap can be judged).
    """
    params = parameters or TappingParameters()
    reference = np.asarray(marked, dtype=np.float64)
    if reference.size < 4:
        raise ValueError("at least four marked taps are needed to score a clip")
    try:
        result: TappingResult = analyse(track, params)
    except AnalysisError as error:
        return Failure(video, str(error))
    tolerance = tolerance_seconds(reference, track.fps)
    found = result.contact_times
    if found.size < 2:
        return Failure(video, "The fingertips were never seen to touch, so no taps can be timed.")
    matching = match_taps(found, reference, tolerance)
    marked_hes, marked_arr = gap_counts(reference, params)
    found_hes, found_arr = gap_counts(found, params)
    return ClipScore(
        video=video,
        marked_count=int(reference.size),
        found_count=int(found.size),
        hits=len(matching.found_index),
        extra=matching.extra,
        missed=matching.missed,
        timing_errors=tuple(float(e) for e in matching.errors),
        marked_rate=_rate(reference),
        found_rate=_rate(found),
        marked_hesitations=marked_hes,
        found_hesitations=found_hes,
        marked_arrests=marked_arr,
        found_arrests=found_arr,
        tolerance=tolerance,
        missing_fraction=result.missing_fraction,
        missed_times=tuple(
            float(t) for j, t in enumerate(reference) if j not in matching.marked_index
        ),
        extra_times=tuple(float(t) for i, t in enumerate(found) if i not in matching.found_index),
    )
