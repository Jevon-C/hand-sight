"""The three movement plug-ins, run the way the server runs them."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

from axonhand.landmarks import LandmarkTrack
from axonhand.synthetic_movements import (
    OpenCloseSpec,
    TremorSpec,
    TurnSpec,
    generate_open_close,
    generate_tremor,
    generate_turns,
)

PLUGINS = Path(__file__).resolve().parents[2] / "plugins"


def _manifest_metrics(name: str) -> list[str]:
    text = (PLUGINS / name / "plugin.toml").read_text()
    return [line.split('"')[1] for line in text.splitlines() if line.startswith("id = ")][1:]


def _run(
    name: str, work: Path, track: LandmarkTrack | None, metrics: list[str]
) -> subprocess.CompletedProcess[str]:
    landmarks = None
    if track is not None:
        landmarks = work / "landmarks.npz"
        track.save(landmarks)
    job = {
        "schema_version": 1,
        "job_id": "j",
        "test_id": name,
        "video_path": "unused.mp4",
        "metrics": metrics,
        "parameters": {},
        "landmarks_path": str(landmarks) if landmarks else None,
    }
    (work / "job.json").write_text(json.dumps(job))
    return subprocess.run(
        [
            sys.executable,
            str(PLUGINS / name / "plugin.py"),
            str(work / "job.json"),
            str(work / "result.json"),
        ],
        capture_output=True,
        text=True,
        cwd=work,
        check=False,
    )


def _values(work: Path) -> dict[str, float]:
    result = json.loads((work / "result.json").read_text())
    return {m["metric_id"]: m["value"] for m in result["measures"]}


def test_open_close_plugin(tmp_path: Path) -> None:
    track, _ = generate_open_close(OpenCloseSpec(rate_hz=2.0, cycles=24, decrement=0.3))
    metrics = [
        "cycle_count",
        "cycle_rate",
        "mean_range",
        "range_variability",
        "range_decrement",
        "hesitations",
        "arrests",
    ]
    done = _run("hand_open_close", tmp_path, track, metrics)
    assert done.returncode == 0, done.stderr
    values = _values(tmp_path)
    assert values["cycle_count"] == 24
    assert values["cycle_rate"] == pytest.approx(2.0, rel=0.02)
    assert values["range_decrement"] == pytest.approx(30.0, abs=3.0)
    signals = json.loads((tmp_path / "signals.json").read_text())
    assert signals["series"][0]["id"] == "openness"
    assert len(signals["events"][0]["times"]) == 24


def test_pronation_plugin(tmp_path: Path) -> None:
    track, truth = generate_turns(TurnSpec(rate_hz=1.5, cycles=12))
    metrics = [
        "turn_count",
        "cycle_rate",
        "excursion_variability",
        "excursion_decrement",
        "hesitations",
        "arrests",
    ]
    done = _run("pronation_supination", tmp_path, track, metrics)
    assert done.returncode == 0, done.stderr
    values = _values(tmp_path)
    assert values["turn_count"] == truth.turn_times.size
    assert values["cycle_rate"] == pytest.approx(1.5, rel=0.02)


def test_tremor_plugin(tmp_path: Path) -> None:
    track = generate_tremor(TremorSpec(frequency_hz=5.0, rms=0.02))
    metrics = [
        "dominant_frequency",
        "amplitude_rms",
        "amplitude_peak_to_peak",
        "band_share",
        "regularity",
    ]
    done = _run("postural_tremor", tmp_path, track, metrics)
    assert done.returncode == 0, done.stderr
    values = _values(tmp_path)
    assert values["dominant_frequency"] == pytest.approx(5.0, abs=0.1)
    assert values["amplitude_rms"] == pytest.approx(0.02, rel=0.05)
    result = json.loads((tmp_path / "result.json").read_text())
    assert {q["status"] for q in result["quality"]} == {"pass"}


@pytest.mark.parametrize("name", ["hand_open_close", "pronation_supination", "postural_tremor"])
def test_a_missing_landmark_file_is_explained_not_crashed(name: str, tmp_path: Path) -> None:
    done = _run(name, tmp_path, None, _manifest_metrics(name)[:1])
    assert done.returncode == 3
    assert "landmarks" in json.loads((tmp_path / "error.json").read_text())["error"]


@pytest.mark.parametrize(
    ("name", "track"),
    [
        ("hand_open_close", generate_open_close(OpenCloseSpec(cycles=3))[0]),
        ("pronation_supination", generate_turns(TurnSpec(cycles=2))[0]),
        ("postural_tremor", generate_tremor(TremorSpec(seconds=2.0))),
    ],
)
def test_too_little_movement_is_explained(name: str, track: LandmarkTrack, tmp_path: Path) -> None:
    done = _run(name, tmp_path, track, _manifest_metrics(name)[:1])
    assert done.returncode == 3
    assert "at least" in json.loads((tmp_path / "error.json").read_text())["error"]


def test_every_manifest_metric_is_computed() -> None:
    """A metric listed in a manifest but missing from the plug-in would fail only at run time."""
    for name in ("hand_open_close", "pronation_supination", "postural_tremor"):
        source = (PLUGINS / name / "plugin.py").read_text()
        for metric in _manifest_metrics(name):
            if metric in {
                "hesitation_factor",
                "arrest_factor",
                "min_range",
                "band_low",
                "band_high",
            }:
                continue
            assert f'"{metric}"' in source, f"{name} does not compute {metric}"
