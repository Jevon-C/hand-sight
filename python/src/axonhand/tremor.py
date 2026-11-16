"""Postural tremor (MDS-UPDRS 3.15): from a landmark track to the measures.

The hand is held out and still, and any small regular movement of the palm is the tremor. The
position of the palm (the mean of the wrist and the four finger bases, which move together) is
taken over time in palm lengths. Its power spectrum is estimated by Welch's method; the tremor
band is 3 to 12 Hz, which holds the usual parkinsonian, essential and physiological tremors.

The measures are the frequency of the strongest peak in the band, the amplitude (root mean square
of the in-band movement), the share of the movement that is in the band, and how regular it is.
A noise floor is estimated from the highest frequencies the frame rate allows, and the quality
check says when the tremor is not clearly above it: a hand model has a little jitter of its own.

Everything here is research-grade measurement, not a clinical score; see ``docs/METRICS.md``.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import PALM_POINTS, LandmarkTrack
from axonhand.tapping import AnalysisError, palm_length

MIN_SECONDS = 5.0
"""Shortest video that gives a spectrum worth reading."""
_SLOW = 1.0
"""Movement below this frequency (Hz) is posture and drift, not tremor."""
_NOISE_GAP = 1.0
"""Frequencies this far above the band (Hz) and higher are taken to hold only tracker noise."""
_MIN_FLOOR_BINS = 3


@dataclass(frozen=True)
class TremorParameters:
    """Settings of the analysis.

    Attributes:
        band_low: lowest tremor frequency, in Hz.
        band_high: highest tremor frequency, in Hz (lowered automatically to stay below the
            frame rate's limit).
        window_seconds: length of each segment of the spectrum estimate.
    """

    band_low: float = 3.0
    band_high: float = 12.0
    window_seconds: float = 2.0

    def __post_init__(self) -> None:
        if not 0.5 <= self.band_low < self.band_high:
            raise ValueError("need 0.5 <= band_low < band_high")
        if self.window_seconds < 1.0:
            raise ValueError("window_seconds must be at least 1")


@dataclass(frozen=True)
class TremorResult:
    """Everything measured from one track."""

    dominant_frequency_hz: float
    amplitude_rms: float
    """Root mean square of the in-band movement of the palm, in palm lengths."""
    amplitude_peak_to_peak: float
    """The peak to peak size of a steady oscillation with that rms: ``2 * sqrt(2) * rms``."""
    band_share_percent: float
    """In-band power as a percentage of all power between 1 Hz and the highest frequency."""
    regularity: float
    """Share of the in-band power within 1 Hz of the strongest peak, 0 to 1."""
    noise_rms: float | None
    """Movement expected in the band from tracker jitter alone, or ``None`` when the frame rate
    leaves no frequencies above the band to estimate it from."""
    frequencies: NDArray[np.float64]
    spectrum: NDArray[np.float64]
    """Power spectral density (palm lengths squared per Hz), x and y added."""
    band: tuple[float, float]
    missing_fraction: float
    duration_seconds: float


def palm_position(track: LandmarkTrack) -> tuple[NDArray[np.float64], float]:
    """Position of the palm in every frame, in palm lengths, and the fraction of frames missing.

    Raises:
        AnalysisError: if the hand size is unknown or more than half the frames are missing.
    """
    palm = palm_length(track)
    points = track.points[:, list(PALM_POINTS), :2]
    found = np.isfinite(points).all(axis=2).sum(axis=1)
    with np.errstate(all="ignore"):
        centre = np.where((found >= len(PALM_POINTS))[:, None], points.mean(axis=1), np.nan)
    missing = ~np.isfinite(centre[:, 0])
    fraction = float(missing.mean())
    if fraction > 0.5:
        raise AnalysisError("The hand was not found in more than half of the video.")
    if missing.any():
        index = np.arange(centre.shape[0])
        for axis in range(2):
            centre[:, axis] = np.interp(index, index[~missing], centre[~missing, axis])
    return centre / palm, fraction


def welch(
    signal: NDArray[np.float64], fps: float, window_seconds: float
) -> tuple[NDArray[np.float64], NDArray[np.float64]]:
    """Power spectral density of a 1-D signal by Welch's method (Hann window, half overlap,
    each segment detrended by a straight line)."""
    size = min(signal.size, max(int(round(window_seconds * fps)), 8))
    step = max(size // 2, 1)
    window = np.hanning(size)
    scale = fps * float(np.sum(window**2))
    total = np.zeros(size // 2 + 1)
    segments = 0
    for start in range(0, signal.size - size + 1, step):
        part = signal[start : start + size]
        x = np.arange(size)
        part = part - np.polyval(np.polyfit(x, part, 1), x)
        power = np.abs(np.fft.rfft(part * window)) ** 2 / scale
        power[1:-1] *= 2.0
        total += power
        segments += 1
    return np.fft.rfftfreq(size, d=1.0 / fps), total / max(segments, 1)


def analyse_tremor(
    track: LandmarkTrack, parameters: TremorParameters | None = None
) -> TremorResult:
    """Measure the tremor of the palm on ``track``.

    Raises:
        AnalysisError: if the hand size is unknown, too much is missing, the video is shorter than
            ``MIN_SECONDS``, or the frame rate is too low to see the tremor band.
    """
    params = parameters or TremorParameters()
    duration = track.frames / track.fps
    if duration < MIN_SECONDS:
        raise AnalysisError(
            f"The video is {duration:.1f} seconds long; at least {MIN_SECONDS:.0f} are needed."
        )
    position, missing = palm_position(track)
    nyquist = track.fps / 2.0
    high = min(params.band_high, 0.9 * nyquist)
    if high <= params.band_low + 1.0:
        raise AnalysisError(
            "The frame rate is too low to see tremor; use 30 frames per second or more."
        )
    freqs, px = welch(position[:, 0], track.fps, params.window_seconds)
    _, py = welch(position[:, 1], track.fps, params.window_seconds)
    spectrum = px + py
    step = float(freqs[1] - freqs[0])
    in_band = (freqs >= params.band_low) & (freqs <= high)
    if not in_band.any():
        raise AnalysisError("The video is too short to resolve the tremor band.")
    band_power = float(spectrum[in_band].sum() * step)
    peak = int(np.argmax(np.where(in_band, spectrum, -np.inf)))
    # Parabolic interpolation around the peak bin for a frequency finer than the bin width.
    frequency = float(freqs[peak])
    if 0 < peak < freqs.size - 1:
        a, b, c = spectrum[peak - 1], spectrum[peak], spectrum[peak + 1]
        denominator = a - 2 * b + c
        if denominator != 0:
            frequency += step * 0.5 * float((a - c) / denominator)
    near = in_band & (np.abs(freqs - frequency) <= 1.0)
    above_slow = freqs >= _SLOW
    total_power = float(spectrum[above_slow].sum() * step)
    noise_rms: float | None = None
    floor = freqs > high + _NOISE_GAP
    if floor.sum() >= _MIN_FLOOR_BINS:
        density = float(spectrum[floor].mean())
        noise_rms = float(np.sqrt(density * (high - params.band_low)))
    rms = float(np.sqrt(band_power))
    return TremorResult(
        dominant_frequency_hz=frequency,
        amplitude_rms=rms,
        amplitude_peak_to_peak=2.0 * float(np.sqrt(2.0)) * rms,
        band_share_percent=100.0 * band_power / total_power if total_power > 0 else 0.0,
        regularity=float(spectrum[near].sum() * step / band_power) if band_power > 0 else 0.0,
        noise_rms=noise_rms,
        frequencies=freqs,
        spectrum=spectrum,
        band=(params.band_low, high),
        missing_fraction=missing,
        duration_seconds=duration,
    )
