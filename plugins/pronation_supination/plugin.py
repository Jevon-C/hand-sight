"""Pronation and supination (MDS-UPDRS 3.6): measures taken from a hand landmark track."""

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
from axonhand.pronation import TurnResult, analyse_turns, face_signal
from axonhand.tapping import AnalysisError, TappingParameters

METRIC_VALUES: dict[str, Callable[[TurnResult], float]] = {
    "turn_count": lambda r: float(r.turn_count),
    "cycle_rate": lambda r: r.cycle_rate_hz,
    "excursion_variability": lambda r: r.excursion_variability_percent,
    "excursion_decrement": lambda r: r.excursion_decrement_percent,
    "hesitations": lambda r: float(r.hesitations),
    "arrests": lambda r: float(r.arrests),
}


def quality(result: TurnResult, duration: float) -> list[QualityItem]:
    """Checks on how far the measures can be trusted."""
    enough = result.turn_count >= 20
    return [
        QualityItem(
            "Number of turns",
            "pass" if enough else "flag",
            f"{result.turn_count} turns found."
            + ("" if enough else " Fewer than 20 makes the slowing measures unreliable."),
        ),
        QualityItem(
            "Knuckles tracked",
            "pass" if result.missing_fraction <= 0.05 else "flag",
            f"{100 * (1 - result.missing_fraction):.0f} percent of frames had the knuckles.",
        ),
        QualityItem(
            "Length of movement",
            "pass" if duration >= 10 else "flag",
            f"{duration:.1f} seconds of turning."
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
            hesitation_factor=job.parameters.get("hesitation_factor", 1.5),
            arrest_factor=job.parameters.get("arrest_factor", 3.0),
        )
        result = analyse_turns(track, parameters)
        signal, _ = face_signal(track)
    except (ValueError, AnalysisError) as error:
        raise PluginError(str(error)) from error
    measures = [Measure(metric, METRIC_VALUES[metric](result)) for metric in job.metrics]
    duration = float(result.turn_times[-1] - result.turn_times[0])
    return PluginResult(
        measures=measures,
        quality=quality(result, duration),
        signals=Signals(
            series=[
                Series(
                    "face",
                    "Which side of the hand faces the camera",
                    "palm lengths squared",
                    0.0,
                    1.0 / track.fps,
                    signal,
                )
            ],
            events=[
                Events(
                    "turns",
                    "Hand edge on (a turn)",
                    "palm lengths squared",
                    result.turn_times,
                    [0.0] * result.turn_times.size,
                )
            ],
        ),
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
