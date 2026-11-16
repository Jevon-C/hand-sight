"""Opening and closing the hand (MDS-UPDRS 3.5): from a landmark track to the measures.

The signal is the mean distance of the five fingertips from the wrist, divided by the palm length.
It is smallest in a fist and largest with the fingers spread. One *cycle* is one opening: a peak
that closes again, found with the same rule as a finger tap. Because a closed fist is not zero
distance, the size of a cycle is its *range*: the peak minus the low points on either side.

Everything here is research-grade measurement, not a clinical score. The definitions are written
in ``docs/METRICS.md``.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import FINGERTIPS, WRIST, LandmarkTrack
from axonhand.tapping import (
    AnalysisError,
    TappingParameters,
    find_peaks,
    gap_counts,
    palm_length,
    smooth,
)

MIN_CYCLES = 4
"""Fewest cycles that give a rate, a spread and a slowing."""
DEFAULT_MIN_RANGE = 0.3
"""Smallest range, in palm lengths, counted as an opening and closing (the fist to the open hand
is about 0.8 to 1.0)."""
_MIN_TIPS = 3


@dataclass(frozen=True)
class OpenCloseResult:
    """Everything measured from one track."""

    peak_times: NDArray[np.float64]
    """Time of each opening's widest point, in seconds."""
    ranges: NDArray[np.float64]
    """Range of each cycle, in palm lengths."""
    cycle_count: int
    cycle_rate_hz: float
    """Cycles per second between the first and last opening."""
    mean_range: float
    range_variability_percent: float
    range_decrement_percent: float
    """Fall of the fitted range from the first to the last cycle; positive means it shrank."""
    hesitations: int
    arrests: int
    missing_fraction: float


def openness_signal(track: LandmarkTrack) -> tuple[NDArray[np.float64], float]:
    """Mean fingertip distance from the wrist in palm lengths, and the fraction of frames missing.

    A frame is missing when fewer than three fingertips, or the wrist, were found. Missing frames
    are filled by straight-line interpolation.

    Raises:
        AnalysisError: if the hand size is unknown or more than half the frames are missing.
    """
    palm = palm_length(track)
    tips = track.points[:, list(FINGERTIPS), :2]
    distances = np.linalg.norm(tips - track.points[:, WRIST : WRIST + 1, :2], axis=2) / palm
    found = np.isfinite(distances).sum(axis=1)
    with np.errstate(all="ignore"):
        raw = np.where(found >= _MIN_TIPS, np.nanmean(distances, axis=1), np.nan)
    missing = ~np.isfinite(raw)
    fraction = float(missing.mean())
    if fraction > 0.5:
        raise AnalysisError("The fingertips were not found in more than half of the video.")
    if missing.any():
        index = np.arange(raw.size)
        raw = np.interp(index, index[~missing], raw[~missing])
    return raw, fraction


def _range_of_each_peak(
    signal: NDArray[np.float64], peaks: NDArray[np.int64]
) -> NDArray[np.float64]:
    """Peak height above the low points on both sides; the first and last peak use only the side
    between peaks, because a video may start or end with the hand held open."""
    smoothed = smooth(signal)
    lows = [float(smoothed[a : b + 1].min()) for a, b in zip(peaks[:-1], peaks[1:], strict=True)]
    ranges = np.empty(peaks.size)
    for k, index in enumerate(peaks):
        height = float(signal[max(index - 1, 0) : index + 2].max())
        sides = [lows[k - 1]] if k > 0 else []
        if k < peaks.size - 1:
            sides.append(lows[k])
        ranges[k] = height - float(np.mean(sides))
    return ranges


def analyse_open_close(
    track: LandmarkTrack, parameters: TappingParameters | None = None
) -> OpenCloseResult:
    """Measure opening and closing of the hand on ``track``.

    ``parameters.min_amplitude`` is the smallest range counted as a cycle; its default for this
    test is :data:`DEFAULT_MIN_RANGE` and is set by the caller.

    Raises:
        AnalysisError: if the hand size is unknown, too much is missing, or fewer than
            ``MIN_CYCLES`` cycles are found.
    """
    params = parameters or TappingParameters(min_amplitude=DEFAULT_MIN_RANGE)
    signal, missing = openness_signal(track)
    # A cycle is an opening above the fist level: the fist is about 1 palm length from the wrist,
    # so the height test is made on the signal with its lowest level taken away.
    level = float(np.percentile(signal, 5))
    peaks = find_peaks(signal - level, track.fps, params.min_amplitude)
    if peaks.size < MIN_CYCLES:
        raise AnalysisError(
            f"Only {peaks.size} openings of the hand were found; at least {MIN_CYCLES} are needed."
        )
    times = peaks / track.fps
    ranges = _range_of_each_peak(signal, peaks)
    hesitations, arrests = gap_counts(times, params)
    order = np.arange(ranges.size, dtype=np.float64)
    slope, intercept = np.polyfit(order, ranges, 1)
    last_fit = intercept + slope * order[-1]
    decrement = 100.0 * (intercept - last_fit) / intercept if intercept > 0 else 0.0
    mean = float(ranges.mean())
    return OpenCloseResult(
        peak_times=times,
        ranges=ranges,
        cycle_count=int(peaks.size),
        cycle_rate_hz=float((peaks.size - 1) / (times[-1] - times[0])),
        mean_range=mean,
        range_variability_percent=float(100.0 * ranges.std() / mean) if mean > 0 else 0.0,
        range_decrement_percent=float(decrement),
        hesitations=hesitations,
        arrests=arrests,
        missing_fraction=missing,
    )
