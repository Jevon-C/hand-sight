"""Postural tremor (MDS-UPDRS 3.15): measures taken from a hand landmark track."""

from __future__ import annotations

from collections.abc import Callable

from axonhand.landmarks import LandmarkTrack
from axonhand.plugin import (
    Job,
    Measure,
    PluginError,
    PluginResult,
    QualityItem,
    Series,
    Signals,
    run,
)
from axonhand.tapping import AnalysisError
from axonhand.tremor import TremorParameters, TremorResult, analyse_tremor, palm_position

METRIC_VALUES: dict[str, Callable[[TremorResult], float]] = {
    "dominant_frequency": lambda r: r.dominant_frequency_hz,
    "amplitude_rms": lambda r: r.amplitude_rms,
    "amplitude_peak_to_peak": lambda r: r.amplitude_peak_to_peak,
    "band_share": lambda r: r.band_share_percent,
    "regularity": lambda r: r.regularity,
}

CLEARLY_ABOVE_NOISE = 3.0
"""Tremor must be this many times the tracker noise to be called clear."""


def quality(result: TremorResult) -> list[QualityItem]:
    """Checks on how far the measures can be trusted."""
    items = [
        QualityItem(
            "Hand tracked",
            "pass" if result.missing_fraction <= 0.05 else "flag",
            f"{100 * (1 - result.missing_fraction):.0f} percent of frames had the palm.",
        ),
        QualityItem(
            "Length of recording",
            "pass" if result.duration_seconds >= 10 else "flag",
            f"{result.duration_seconds:.1f} seconds of recording."
            + ("" if result.duration_seconds >= 10 else " Ten seconds or more is advised."),
        ),
    ]
    if result.noise_rms is None:
        items.append(
            QualityItem(
                "Above tracker noise",
                "flag",
                "The frame rate leaves no frequencies above the tremor band, so the tracker "
                "noise could not be estimated. Film at 60 frames per second.",
            )
        )
    else:
        ratio = result.amplitude_rms / result.noise_rms if result.noise_rms > 0 else float("inf")
        clear = ratio >= CLEARLY_ABOVE_NOISE
        items.append(
            QualityItem(
                "Above tracker noise",
                "pass" if clear else "flag",
                (
                    f"The tremor is {ratio:.0f} times the tracker noise."
                    if clear
                    else "The movement in the tremor band is close to what the tracker alone "
                    "would show, so it may not be a tremor."
                ),
            )
        )
    return items


def compute(job: Job) -> PluginResult:
    """Analyse the landmark track named in the job."""
    if job.landmarks_path is None or not job.landmarks_path.is_file():
        raise PluginError("No hand landmarks were found for this video.")
    try:
        track = LandmarkTrack.load(job.landmarks_path)
        parameters = TremorParameters(
            band_low=job.parameters.get("band_low", 3.0),
            band_high=job.parameters.get("band_high", 12.0),
        )
        result = analyse_tremor(track, parameters)
        position, _ = palm_position(track)
    except (ValueError, AnalysisError) as error:
        raise PluginError(str(error)) from error
    measures = [Measure(metric, METRIC_VALUES[metric](result)) for metric in job.metrics]
    movement = position - position.mean(axis=0)
    # Show the movement along the direction in which the palm moves most.
    axis = movement[:, 0] if movement[:, 0].std() >= movement[:, 1].std() else movement[:, 1]
    return PluginResult(
        measures=measures,
        quality=quality(result),
        signals=Signals(
            series=[Series("palm", "Palm movement", "palm lengths", 0.0, 1.0 / track.fps, axis)]
        ),
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
