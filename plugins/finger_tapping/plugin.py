"""Finger tapping (MDS-UPDRS 3.4): measures taken from a hand landmark track."""

from __future__ import annotations

from collections.abc import Callable

from axonhand.landmarks import LandmarkTrack
from axonhand.plugin import (
    Events,
    Job,
    Measure,
    PluginError,
    PluginResult,
    QualityItem,
    Series,
    Signals,
    run,
)
from axonhand.tapping import (
    AnalysisError,
    TappingParameters,
    TappingResult,
    analyse,
    opening_signal,
)

METRIC_VALUES: dict[str, Callable[[TappingResult], float]] = {
    "tap_count": lambda r: float(r.tap_count),
    "tap_rate": lambda r: r.tap_rate_hz,
    "mean_amplitude": lambda r: r.mean_amplitude,
    "amplitude_variability": lambda r: r.amplitude_variability_percent,
    "amplitude_decrement": lambda r: r.amplitude_decrement_percent,
    "hesitations": lambda r: float(r.hesitations),
    "arrests": lambda r: float(r.arrests),
}


def quality(result: TappingResult, duration: float) -> list[QualityItem]:
    """Checks on how far the measures can be trusted."""
    items = [
        QualityItem(
            "Number of taps",
            "pass" if result.tap_count >= 10 else "flag",
            f"{result.tap_count} taps found."
            + (
                ""
                if result.tap_count >= 10
                else " Fewer than 10 makes the slowing measures unreliable."
            ),
        ),
        QualityItem(
            "Fingertips tracked",
            "pass" if result.missing_fraction <= 0.05 else "flag",
            f"{100 * (1 - result.missing_fraction):.0f} percent of frames had both fingertips.",
        ),
        QualityItem(
            "Length of tapping",
            "pass" if duration >= 10 else "flag",
            f"{duration:.1f} seconds of tapping."
            + ("" if duration >= 10 else " Ten seconds or more is advised."),
        ),
    ]
    return items


def compute(job: Job) -> PluginResult:
    """Analyse the landmark track named in the job."""
    if job.landmarks_path is None or not job.landmarks_path.is_file():
        raise PluginError("No hand landmarks were found for this video.")
    try:
        track = LandmarkTrack.load(job.landmarks_path)
        parameters = TappingParameters(
            min_amplitude=job.parameters.get("min_amplitude", 0.15),
            hesitation_factor=job.parameters.get("hesitation_factor", 1.5),
            arrest_factor=job.parameters.get("arrest_factor", 3.0),
        )
        result = analyse(track, parameters)
    except (ValueError, AnalysisError) as error:
        raise PluginError(str(error)) from error
    measures = [Measure(metric, METRIC_VALUES[metric](result)) for metric in job.metrics]
    opening, _ = opening_signal(track)
    duration = float(result.peak_times[-1] - result.peak_times[0])
    return PluginResult(
        measures=measures,
        quality=quality(result, duration),
        signals=Signals(
            series=[
                Series(
                    "opening",
                    "Thumb to index distance",
                    "palm lengths",
                    0.0,
                    1.0 / track.fps,
                    opening,
                )
            ],
            events=[Events("taps", "Taps", "palm lengths", result.peak_times, result.amplitudes)],
        ),
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
