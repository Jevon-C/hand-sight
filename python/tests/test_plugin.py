"""Tests for the plug-in helpers."""

import json
import math
from pathlib import Path

import pytest

from axonhand import plugin
from axonhand.plugin import Job, Measure, PluginError, PluginResult


def write_job(path: Path, **overrides: object) -> Path:
    job: dict[str, object] = {
        "schema_version": 1,
        "job_id": "j1",
        "test_id": "t",
        "video_path": "/v.mp4",
        "metrics": ["a", "b"],
        "parameters": {"amp": 0.5},
    }
    job.update(overrides)
    path.write_text(json.dumps(job))
    return path


def test_job_is_read_with_typed_fields(tmp_path: Path) -> None:
    job = plugin.read_job(write_job(tmp_path / "job.json"))
    assert job == Job("j1", "t", Path("/v.mp4"), ("a", "b"), {"amp": 0.5})


@pytest.mark.parametrize(
    "bad",
    [{"schema_version": 2}, {"metrics": 5}, {"parameters": {"x": "word"}}],
)
def test_unreadable_jobs_raise_plugin_error(tmp_path: Path, bad: dict[str, object]) -> None:
    with pytest.raises(PluginError):
        plugin.read_job(write_job(tmp_path / "job.json", **bad))


def test_missing_and_malformed_job_files_raise_plugin_error(tmp_path: Path) -> None:
    with pytest.raises(PluginError):
        plugin.read_job(tmp_path / "absent.json")
    junk = tmp_path / "junk.json"
    junk.write_text("not json")
    with pytest.raises(PluginError):
        plugin.read_job(junk)


def test_a_measure_without_a_value_needs_a_reason() -> None:
    with pytest.raises(ValueError, match="reason"):
        Measure("a", None)
    assert Measure("a", None, "no taps found").value is None


@pytest.mark.parametrize("bad", [math.nan, math.inf, -math.inf])
def test_a_measure_must_be_finite(bad: float) -> None:
    with pytest.raises(ValueError, match="finite"):
        Measure("a", bad)


def test_run_writes_the_result_file(tmp_path: Path) -> None:
    job = write_job(tmp_path / "job.json")
    out = tmp_path / "result.json"

    def compute(job: Job) -> PluginResult:
        return PluginResult([Measure(m, 1.0) for m in job.metrics], notes=["ok"])

    assert plugin.run(compute, [str(job), str(out)]) == 0
    written = json.loads(out.read_text())
    assert written["schema_version"] == 1
    assert [m["metric_id"] for m in written["measures"]] == ["a", "b"]
    assert written["notes"] == ["ok"]


def test_plugin_error_is_explained_in_error_json(tmp_path: Path) -> None:
    job = write_job(tmp_path / "job.json")

    def compute(job: Job) -> PluginResult:
        raise PluginError("No hand was visible in the video.")

    code = plugin.run(compute, [str(job), str(tmp_path / "result.json")])
    assert code == plugin.EXIT_EXPLAINED_FAILURE
    assert json.loads((tmp_path / "error.json").read_text()) == {
        "error": "No hand was visible in the video."
    }
    assert not (tmp_path / "result.json").exists()


def test_unexpected_exceptions_propagate_so_the_traceback_is_logged(tmp_path: Path) -> None:
    job = write_job(tmp_path / "job.json")

    def compute(job: Job) -> PluginResult:
        raise ZeroDivisionError

    with pytest.raises(ZeroDivisionError):
        plugin.run(compute, [str(job), str(tmp_path / "result.json")])


def test_wrong_argument_count_is_a_usage_error() -> None:
    assert plugin.run(lambda job: PluginResult([]), ["only-one"]) == plugin.EXIT_BAD_USAGE
