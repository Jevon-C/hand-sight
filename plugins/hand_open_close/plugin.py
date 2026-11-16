"""Opening and closing the hand (MDS-UPDRS 3.5): measures taken from a hand landmark track."""

from __future__ import annotations

from collections.abc import Callable

from axonhand.landmarks import LandmarkTrack
from axonhand.movement import (
    DEFAULT_MIN_RANGE,
    OpenCloseResult,
    analyse_open_close,
    openness_signal,
)
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
from axonhand.tapping import AnalysisError, TappingParameters

METRIC_VALUES: dict[str, Callable[[OpenCloseResult], float]] = {
    "cycle_count": lambda r: float(r.cycle_count),
    "cycle_rate": lambda r: r.cycle_rate_hz,
    "mean_range": lambda r: r.mean_range,
    "range_variability": lambda r: r.range_variability_percent,
    "range_decrement": lambda r: r.range_decrement_percent,
    "hesitations": lambda r: float(r.hesitations),
    "arrests": lambda r: float(r.arrests),
}


def quality(result: OpenCloseResult, duration: float) -> list[QualityItem]:
    """Checks on how far the measures can be trusted."""
    enough = result.cycle_count >= 10
    return [
        QualityItem(
            "Number of cycles",
            "pass" if enough else "flag",
            f"{result.cycle_count} openings found."
            + ("" if enough else " Fewer than 10 makes the slowing measures unreliable."),
        ),
        QualityItem(
            "Fingertips tracked",
            "pass" if result.missing_fraction <= 0.05 else "flag",
            f"{100 * (1 - result.missing_fraction):.0f} percent of frames had the fingertips.",
        ),
        QualityItem(
            "Length of movement",
            "pass" if duration >= 10 else "flag",
            f"{duration:.1f} seconds of opening and closing."
            + ("" if duration >= 10 else " Ten seconds or more is advised."),
        ),
    ]


def compute(job: Job) -> PluginResult:
    """Analyse the landmark track named in the job."""
    if job.landmarks_path is None or not job.landmarks_path.is_file():
        raise PluginError("No hand landmarks were found for this video.")
    try:
        track = LandmarkTrack.load(job.landmarks_path)
        parameters = TappingParameters(
            min_amplitude=job.parameters.get("min_range", DEFAULT_MIN_RANGE),
            hesitation_factor=job.parameters.get("hesitation_factor", 1.5),
            arrest_factor=job.parameters.get("arrest_factor", 3.0),
        )
        result = analyse_open_close(track, parameters)
        signal, _ = openness_signal(track)
    except (ValueError, AnalysisError) as error:
        raise PluginError(str(error)) from error
    measures = [Measure(metric, METRIC_VALUES[metric](result)) for metric in job.metrics]
    duration = float(result.peak_times[-1] - result.peak_times[0])
    return PluginResult(
        measures=measures,
        quality=quality(result, duration),
        signals=Signals(
            series=[
                Series(
                    "openness",
                    "Fingertip distance from the wrist",
                    "palm lengths",
                    0.0,
                    1.0 / track.fps,
                    signal,
                )
            ],
            events=[
                Events("openings", "Openings", "palm lengths", result.peak_times, result.ranges)
            ],
        ),
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
