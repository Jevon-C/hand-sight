"""Finger tapping analysis: from a landmark track to the measures of the test.

The signal is the distance between the thumb tip and the index fingertip, divided by the palm
length so it does not depend on how far the hand is from the camera. Each *tap* is one opening
of the fingers: a peak in that signal. Measures follow the usual description of the task
(rate, size, slowing, hesitations, arrests); how they are defined here is written in
``docs/METRICS.md``, and is checked against the literature in phase 5.

Everything here is research-grade measurement, not a clinical score.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import INDEX_TIP, MIDDLE_MCP, THUMB_TIP, WRIST, LandmarkTrack


class AnalysisError(Exception):
    """The track cannot be analysed. The message is written for a clinician."""


@dataclass(frozen=True)
class TappingParameters:
    """Settings of the analysis.

    Attributes:
        min_amplitude: smallest opening counted as a tap, in palm lengths.
        hesitation_factor: an interval longer than this many typical intervals is a hesitation.
        arrest_factor: an interval longer than this many typical intervals is an arrest
            (counted as an arrest, not as a hesitation).
    """

    min_amplitude: float = 0.15
    hesitation_factor: float = 1.5
    arrest_factor: float = 3.0

    def __post_init__(self) -> None:
        if self.min_amplitude <= 0:
            raise ValueError("min_amplitude must be positive")
        if not 1.0 < self.hesitation_factor < self.arrest_factor:
            raise ValueError("need 1 < hesitation_factor < arrest_factor")


@dataclass(frozen=True)
class TappingResult:
    """Everything measured from one track."""

    peak_times: NDArray[np.float64]
    """Time of each tap's widest opening, in seconds."""
    amplitudes: NDArray[np.float64]
    """Widest opening of each tap, in palm lengths."""
    tap_count: int
    tap_rate_hz: float
    """Taps per second between the first and last tap."""
    mean_amplitude: float
    amplitude_variability_percent: float
    """Standard deviation of the amplitudes as a percentage of their mean."""
    amplitude_decrement_percent: float
    """Fall of the fitted amplitude from the first to the last tap; positive means it shrank."""
    hesitations: int
    arrests: int
    missing_fraction: float
    """Fraction of frames in which a landmark needed for the signal was missing."""
    contact_times: NDArray[np.float64] = field(default_factory=lambda: np.empty(0))
    """Time at which the fingertips touch, for each closing of the fingers that was filmed."""


MIN_TAPS = 4


def palm_length(track: LandmarkTrack) -> float:
    """Median wrist to middle-knuckle distance over the frames where both were found."""
    spans = np.linalg.norm(track.points[:, WRIST, :2] - track.points[:, MIDDLE_MCP, :2], axis=1)
    spans = spans[np.isfinite(spans)]
    if spans.size == 0 or np.median(spans) <= 0:
        raise AnalysisError(
            "The wrist and the knuckle were never found, so the hand size is unknown."
        )
    return float(np.median(spans))


def opening_signal(track: LandmarkTrack) -> tuple[NDArray[np.float64], float]:
    """The thumb to index distance in palm lengths for every frame, and the fraction missing.

    Frames where a fingertip was not found are filled by straight-line interpolation, because
    a tip that is hidden behind the other one is the closed position.
    """
    palm = palm_length(track)
    raw = (
        np.linalg.norm(track.points[:, THUMB_TIP, :2] - track.points[:, INDEX_TIP, :2], axis=1)
        / palm
    )
    missing = ~np.isfinite(raw)
    fraction = float(missing.mean())
    if fraction > 0.5:
        raise AnalysisError("The fingertips were not found in more than half of the video.")
    if missing.any():
        index = np.arange(raw.size)
        raw = np.interp(index, index[~missing], raw[~missing])
    return raw, fraction


Smoother = Callable[[NDArray[np.float64]], NDArray[np.float64]]
"""Turns the opening signal into a smoother one of the same length."""


def _smooth(signal: NDArray[np.float64]) -> NDArray[np.float64]:
    """Three-point moving average; keeps the peaks where they are."""
    padded = np.pad(signal, 1, mode="edge")
    return np.asarray((padded[:-2] + padded[1:-1] + padded[2:]) / 3.0, dtype=np.float64)


smooth = _smooth
"""The smoothing the analyses share, under a public name."""


def _dominant_period(signal: NDArray[np.float64], fps: float) -> float:
    """Seconds per tap, from the strongest frequency between 0.5 and 10 Hz."""
    centred = signal - signal.mean()
    spectrum = np.abs(np.fft.rfft(centred * np.hanning(centred.size)))
    freqs = np.fft.rfftfreq(centred.size, d=1.0 / fps)
    band = (freqs >= 0.5) & (freqs <= 10.0)
    if not band.any() or spectrum[band].max() <= 0:
        raise AnalysisError("No regular tapping was found in the video.")
    return float(1.0 / freqs[band][np.argmax(spectrum[band])])


def _prominence(signal: NDArray[np.float64], index: int) -> float:
    """How far the point at ``index`` stands above its surroundings.

    The height above the higher of the two lowest points reached on either side before the
    signal rises above this point. A bump on a held-open plateau has almost none; a tap that
    opens and closes again has nearly its full height. When the video ends before one side
    reaches any higher point, that side is not judged (the tap's closing may simply not have been
    filmed) and the other side decides.
    """
    value = float(signal[index])
    bases: list[float] = []
    for side in (signal[:index][::-1], signal[index + 1 :]):
        higher = np.nonzero(side > value)[0]
        reach = side[: higher[0]] if higher.size else side
        base = min(value, float(reach.min())) if reach.size else value
        bases.append(base if higher.size else -np.inf)
    judged = [b for b in bases if np.isfinite(b)]
    if not judged:
        # Nothing is higher on either side: the highest point of the whole signal. It stands out
        # if it does on either side (a flat run to the end of the video says nothing).
        judged = [
            min(value, float(signal[:index].min()) if index else value),
            min(value, float(signal[index + 1 :].min()) if index + 1 < signal.size else value),
        ]
        return value - min(judged)
    return value - max(judged)


CLOSING_FRACTION = 0.5
"""A tap must also close again by at least this fraction of ``min_amplitude``."""


def _pick(
    signal: NDArray[np.float64],
    fps: float,
    min_height: float,
    min_prominence: float,
    smoother: Smoother = _smooth,
) -> NDArray[np.int64]:
    """Local maxima of ``signal`` of at least ``min_height`` that stand at least
    ``min_prominence`` above their surroundings, at least half a period apart (the higher wins)."""
    smooth = smoother(signal)
    interior = np.arange(1, smooth.size - 1)
    is_peak = (smooth[interior] > smooth[interior - 1]) & (smooth[interior] >= smooth[interior + 1])
    candidates = np.array(
        [
            i
            for i in interior[is_peak]
            if smooth[i] >= min_height and _prominence(smooth, int(i)) >= min_prominence
        ],
        dtype=np.int64,
    )
    if candidates.size == 0:
        return np.array([], dtype=np.int64)
    min_gap = 0.5 * _dominant_period(signal, fps) * fps
    kept: list[int] = []
    for index in candidates[np.argsort(-smooth[candidates])]:
        if all(abs(int(index) - other) >= min_gap for other in kept):
            kept.append(int(index))
    return np.array(sorted(kept), dtype=np.int64)


def find_peaks(
    signal: NDArray[np.float64],
    fps: float,
    min_amplitude: float,
    smoother: Smoother = _smooth,
) -> NDArray[np.int64]:
    """Indices of the taps: local maxima at least ``min_amplitude`` high that also close again by
    at least ``CLOSING_FRACTION`` of that, and are at least half a period apart.

    Closing again is what makes a tap: a hand held open with small wobbles has no taps. When
    two maxima are closer than half a period, the higher one is kept.
    """
    return _pick(signal, fps, min_amplitude, CLOSING_FRACTION * min_amplitude, smoother)


TOUCH_FRACTION = 0.1
"""The fingertips count as touching once the opening has fallen to within this fraction of the
way from the preceding high point down to the lowest point of the closing."""


def contact_frames(
    signal: NDArray[np.float64],
    fps: float,
    min_amplitude: float,
    smoother: Smoother = _smooth,
) -> NDArray[np.int64]:
    """The frame at which the fingertips touch, for each closing of the fingers.

    A closing is a low point of the opening that stands at least ``CLOSING_FRACTION`` of
    ``min_amplitude`` below its surroundings. Its contact is the
    first frame, after the preceding high point, at which the opening has fallen to within
    ``TOUCH_FRACTION`` of the way down to the low point: the moment of touching, not the middle
    of a pause with the fingers together. A closing that begins from a held-open hand counts, and
    a closing the video ends before completing does not.
    """
    valleys = _pick(-signal, fps, -np.inf, CLOSING_FRACTION * min_amplitude, smoother)
    smooth = smoother(signal)
    contacts: list[int] = []
    previous = 0
    for valley in valleys:
        valley = int(valley)
        top = previous + int(np.argmax(smooth[previous : valley + 1]))
        low = float(smooth[valley])
        level = low + TOUCH_FRACTION * (float(smooth[top]) - low)
        contacts.append(top + int(np.argmax(smooth[top : valley + 1] <= level)))
        previous = valley
    return np.array(contacts, dtype=np.int64)


def gap_counts(times: NDArray[np.float64], parameters: TappingParameters) -> tuple[int, int]:
    """Count the ``(hesitations, arrests)`` among the gaps between consecutive tap times.

    A gap is judged against the median gap. The same rule is applied to the taps found by the
    software and to taps marked by a person, so the two are comparable.
    """
    intervals = np.diff(times)
    if intervals.size == 0:
        return 0, 0
    typical = float(np.median(intervals))
    arrests = int((intervals > parameters.arrest_factor * typical).sum())
    hesitations = int(
        (
            (intervals > parameters.hesitation_factor * typical)
            & (intervals <= parameters.arrest_factor * typical)
        ).sum()
    )
    return hesitations, arrests


def analyse(
    track: LandmarkTrack,
    parameters: TappingParameters | None = None,
    smoother: Smoother = _smooth,
) -> TappingResult:
    """Measure finger tapping on ``track``.

    Raises:
        AnalysisError: if the hand size is unknown, too much is missing, or fewer than
            ``MIN_TAPS`` taps are found.
    """
    params = parameters or TappingParameters()
    signal, missing = opening_signal(track)
    peaks = find_peaks(signal, track.fps, params.min_amplitude, smoother)
    if peaks.size < MIN_TAPS:
        raise AnalysisError(
            f"Only {peaks.size} finger taps were found; at least {MIN_TAPS} are needed."
        )
    times = peaks / track.fps
    # The height of a tap is read from the unsmoothed signal around the smoothed peak.
    amplitudes = np.array([signal[max(i - 1, 0) : i + 2].max() for i in peaks])
    hesitations, arrests = gap_counts(times, params)
    order = np.arange(amplitudes.size, dtype=np.float64)
    slope, intercept = np.polyfit(order, amplitudes, 1)
    first_fit = intercept
    last_fit = intercept + slope * order[-1]
    decrement = 100.0 * (first_fit - last_fit) / first_fit if first_fit > 0 else 0.0
    mean = float(amplitudes.mean())
    return TappingResult(
        peak_times=times,
        amplitudes=amplitudes,
        tap_count=int(peaks.size),
        tap_rate_hz=float((peaks.size - 1) / (times[-1] - times[0])),
        mean_amplitude=mean,
        amplitude_variability_percent=float(100.0 * amplitudes.std() / mean),
        amplitude_decrement_percent=float(decrement),
        hesitations=hesitations,
        arrests=arrests,
        missing_fraction=missing,
        contact_times=contact_frames(signal, track.fps, params.min_amplitude, smoother) / track.fps,
    )
