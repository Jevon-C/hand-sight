"""The finger tapping plug-in, run the way the server runs it."""

import importlib.util
import json
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pytest

from axonhand.synthetic import TapSpec, generate_tapping

PLUGIN = Path(__file__).resolve().parents[2] / "plugins" / "finger_tapping" / "plugin.py"
ALL_METRICS = [
    "tap_count",
    "tap_rate",
    "mean_amplitude",
    "amplitude_variability",
    "amplitude_decrement",
    "hesitations",
    "arrests",
]


def run_plugin(
    work: Path, landmarks: Path | None, metrics: list[str], **parameters: float
) -> subprocess.CompletedProcess[str]:
    job = {
        "schema_version": 1,
        "job_id": "j",
        "test_id": "finger_tapping",
        "video_path": "unused.mp4",
        "metrics": metrics,
        "parameters": parameters,
        "landmarks_path": str(landmarks) if landmarks else None,
    }
    (work / "job.json").write_text(json.dumps(job))
    return subprocess.run(
        [sys.executable, str(PLUGIN), str(work / "job.json"), str(work / "result.json")],
        capture_output=True,
        text=True,
        cwd=work,
        check=False,
    )


def test_a_real_run_returns_truthful_measures_and_signals(tmp_path: Path) -> None:
    data = generate_tapping(TapSpec(rate_hz=3.0, taps=36, amplitude=0.8, decrement=0.3))
    landmarks = tmp_path / "landmarks.npz"
    data.track.save(landmarks)
    done = run_plugin(tmp_path, landmarks, ALL_METRICS)
    assert done.returncode == 0, done.stderr
    result = json.loads((tmp_path / "result.json").read_text())
    values = {m["metric_id"]: m["value"] for m in result["measures"]}
    assert values["tap_count"] == 36
    assert values["tap_rate"] == pytest.approx(3.0, rel=0.02)
    assert values["amplitude_decrement"] == pytest.approx(30.0, abs=3.0)
    assert {q["status"] for q in result["quality"]} == {"pass"}
    signals = json.loads((tmp_path / "signals.json").read_text())
    assert signals["series"][0]["id"] == "opening"
    assert len(signals["events"][0]["times"]) == 36


def test_only_requested_metrics_are_returned(tmp_path: Path) -> None:
    landmarks = tmp_path / "landmarks.npz"
    generate_tapping(TapSpec(taps=12)).track.save(landmarks)
    assert run_plugin(tmp_path, landmarks, ["tap_rate"]).returncode == 0
    result = json.loads((tmp_path / "result.json").read_text())
    assert [m["metric_id"] for m in result["measures"]] == ["tap_rate"]


def test_few_taps_are_flagged_not_hidden(tmp_path: Path) -> None:
    landmarks = tmp_path / "landmarks.npz"
    generate_tapping(TapSpec(taps=6)).track.save(landmarks)
    assert run_plugin(tmp_path, landmarks, ["tap_count"]).returncode == 0
    result = json.loads((tmp_path / "result.json").read_text())
    assert any(q["status"] == "flag" for q in result["quality"])


@pytest.mark.parametrize("case", ["missing", "no_taps"])
def test_failures_are_explained(tmp_path: Path, case: str) -> None:
    landmarks: Path | None = None
    if case == "no_taps":
        landmarks = tmp_path / "landmarks.npz"
        generate_tapping(TapSpec(taps=20, amplitude=0.05)).track.save(landmarks)
    done = run_plugin(tmp_path, landmarks, ["tap_count"])
    assert done.returncode == 3
    assert json.loads((tmp_path / "error.json").read_text())["error"]
    assert not (tmp_path / "result.json").exists()


def test_the_plugin_module_exposes_a_value_for_every_manifest_metric() -> None:
    spec = importlib.util.spec_from_file_location("ft_plugin", PLUGIN)
    assert spec is not None and spec.loader is not None
    module: ModuleType = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert set(module.METRIC_VALUES) == set(ALL_METRICS)
