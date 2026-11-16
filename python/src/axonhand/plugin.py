"""Helpers for writing a test plug-in.

A plug-in is a folder with a ``plugin.toml`` and one script. The script defines one function
that takes a :class:`Job` and returns a :class:`PluginResult`, then hands it to :func:`run`::

    from axonhand.plugin import Job, Measure, PluginResult, run

    def compute(job: Job) -> PluginResult:
        return PluginResult(measures=[Measure("tap_rate", 3.2)])

    if __name__ == "__main__":
        raise SystemExit(run(compute))

The server starts the script with two arguments, the job file and the path for the result
file. ``run`` reads the first, calls ``compute`` and writes the second. If ``compute`` raises
:class:`PluginError`, its message is saved to ``error.json`` and shown to the user; any other
exception prints a traceback to the log and is shown only as a generic failure. The file
formats are described in ``docs/PLUGIN-PROTOCOL.md``.
"""

from __future__ import annotations

import json
import math
import sys
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import numpy as np
from numpy.typing import NDArray

SCHEMA_VERSION = 1
"""Version of the job and result file formats."""

EXIT_EXPLAINED_FAILURE = 3
EXIT_BAD_USAGE = 2
EXIT_UNEXPECTED = 1


class PluginError(Exception):
    """A failure the user should read, for example "No hand was visible in the video."

    The message is shown in the app, so write it for a clinician, not a programmer, and never
    include file paths or personal data.
    """


@dataclass(frozen=True)
class Job:
    """What the server asks the plug-in to do."""

    job_id: str
    test_id: str
    video_path: Path
    metrics: tuple[str, ...]
    parameters: Mapping[str, float]
    landmarks_path: Path | None = None
    """Landmark track (``.npz``) found in the video by the hand model, when there is one."""


@dataclass(frozen=True)
class Measure:
    """One number reported for a metric.

    ``value`` is ``None`` when the metric cannot be measured on this video; ``reason`` must
    then say why.
    """

    metric_id: str
    value: float | None
    reason: str | None = None

    def __post_init__(self) -> None:
        if self.value is None and not self.reason:
            raise ValueError(f"{self.metric_id!r} has no value, so it needs a reason")
        if self.value is not None and not math.isfinite(self.value):
            raise ValueError(f"{self.metric_id!r} must be a finite number")


@dataclass(frozen=True)
class QualityItem:
    """One check on how trustworthy the measures are.

    ``status`` is ``"pass"``, ``"flag"`` (read with care) or ``"fail"``.
    """

    name: str
    status: str
    detail: str

    def __post_init__(self) -> None:
        if self.status not in ("pass", "flag", "fail"):
            raise ValueError("status must be 'pass', 'flag' or 'fail'")


@dataclass(frozen=True)
class Series:
    """A signal sampled at a fixed rate, for drawing a plot."""

    id: str
    name: str
    unit: str
    start_seconds: float
    step_seconds: float
    values: Sequence[float] | NDArray[np.float64]


@dataclass(frozen=True)
class Events:
    """Marked moments, each with a value, for drawing on a plot (for example one per tap)."""

    id: str
    name: str
    unit: str
    times: Sequence[float] | NDArray[np.float64]
    values: Sequence[float] | NDArray[np.float64]

    def __post_init__(self) -> None:
        if len(self.times) != len(self.values):
            raise ValueError("times and values must have the same length")


@dataclass(frozen=True)
class Signals:
    """Data behind the plots of a result."""

    series: Sequence[Series] = field(default_factory=tuple)
    events: Sequence[Events] = field(default_factory=tuple)


@dataclass(frozen=True)
class PluginResult:
    """Everything a plug-in returns for one run."""

    measures: Sequence[Measure]
    notes: Sequence[str] = field(default_factory=tuple)
    quality: Sequence[QualityItem] = field(default_factory=tuple)
    signals: Signals | None = None


Compute = Callable[[Job], PluginResult]


def read_job(path: Path) -> Job:
    """Read the job file written by the server.

    Raises:
        PluginError: if the file is not a job this version of the helpers understands.
    """
    try:
        raw: Any = json.loads(path.read_text(encoding="utf-8"))
        if raw["schema_version"] != SCHEMA_VERSION:
            raise PluginError(f"unsupported job format version {raw['schema_version']}")
        return Job(
            job_id=str(raw["job_id"]),
            test_id=str(raw["test_id"]),
            video_path=Path(raw["video_path"]),
            metrics=tuple(str(m) for m in raw["metrics"]),
            parameters={str(k): float(v) for k, v in raw["parameters"].items()},
            landmarks_path=Path(raw["landmarks_path"]) if raw.get("landmarks_path") else None,
        )
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as error:
        raise PluginError("the job file could not be read") from error


def result_to_json(result: PluginResult) -> dict[str, Any]:
    """The result as the JSON object the server expects."""
    return {
        "schema_version": SCHEMA_VERSION,
        "measures": [
            {"metric_id": m.metric_id, "value": m.value, "reason": m.reason}
            for m in result.measures
        ],
        "notes": list(result.notes),
        "quality": [
            {"name": q.name, "status": q.status, "detail": q.detail} for q in result.quality
        ],
    }


def signals_to_json(signals: Signals) -> dict[str, Any]:
    """The signals as the JSON object written to ``signals.json``."""
    return {
        "schema_version": SCHEMA_VERSION,
        "series": [
            {
                "id": s.id,
                "name": s.name,
                "unit": s.unit,
                "start_seconds": s.start_seconds,
                "step_seconds": s.step_seconds,
                "values": [float(v) for v in s.values],
            }
            for s in signals.series
        ],
        "events": [
            {
                "id": e.id,
                "name": e.name,
                "unit": e.unit,
                "times": [float(t) for t in e.times],
                "values": [float(v) for v in e.values],
            }
            for e in signals.events
        ],
    }


def run(compute: Compute, argv: Sequence[str] | None = None) -> int:
    """Run ``compute`` as the server expects and return the process exit code."""
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 2:
        print("usage: plugin.py JOB_FILE RESULT_FILE", file=sys.stderr)
        return EXIT_BAD_USAGE
    job_path, result_path = Path(args[0]), Path(args[1])
    try:
        result = compute(read_job(job_path))
        if result.signals is not None:
            (result_path.parent / "signals.json").write_text(
                json.dumps(signals_to_json(result.signals)), encoding="utf-8"
            )
        result_path.write_text(json.dumps(result_to_json(result)), encoding="utf-8")
    except PluginError as error:
        (result_path.parent / "error.json").write_text(
            json.dumps({"error": str(error)}), encoding="utf-8"
        )
        return EXIT_EXPLAINED_FAILURE
    return 0
