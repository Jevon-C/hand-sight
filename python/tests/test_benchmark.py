"""The benchmark tool runs on the real models when they are present."""

from pathlib import Path

import pytest

from axonhand.synthetic import TapSpec, generate_tapping, render_video
from axonhand.tools import benchmark

MODELS = Path(__file__).resolve().parents[2] / "models"
needs_models = pytest.mark.skipif(
    not (MODELS / "palm_detection_mediapipe_2023feb.onnx").is_file(), reason="models not present"
)


def test_the_table_lists_every_device_and_leaves_out_memory_when_unknown() -> None:
    timing = benchmark.ModelTiming(1.0, 2.0, 3.0)
    report = benchmark.DeviceReport(
        "NPU/NPU", timing, timing, 100, 2.0, 0.1, 1.0, 3.5, 285.0, 1, 4.2, ("a note",)
    )
    text = benchmark.markdown("c.mp4", [report], {"machine": "arm64"}, None)
    assert "NPU/NPU" in text and "285" in text and "Peak memory" not in text and "a note" in text


@needs_models
def test_each_model_is_timed_on_its_own_input_shape() -> None:
    from axonhand.hand.runtime import open_model

    for name in (
        "palm_detection_mediapipe_2023feb.onnx",
        "handpose_estimation_mediapipe_2023feb.onnx",
    ):
        timing = benchmark.inference_timing(open_model(MODELS / name, "cpu"), MODELS / name, runs=3)
        assert 0 < timing.median_ms <= timing.p95_ms and timing.file_mb > 1


@needs_models
def test_a_video_without_a_hand_is_reported_not_benchmarked(tmp_path: Path) -> None:
    video = tmp_path / "c.mp4"
    render_video(generate_tapping(TapSpec(taps=6, fps=30.0)).track, video)
    code = benchmark.main(
        [str(video), "--models", str(MODELS), "--runs", "3", "--out", str(tmp_path)]
    )
    assert code == 1


def test_the_pipeline_is_timed_stage_by_stage_with_stand_in_models(tmp_path: Path) -> None:
    from test_hand_tracker import StubLandmarks, StubPalm

    video = tmp_path / "c.mp4"
    render_video(generate_tapping(TapSpec(taps=8, rate_hz=2.0, fps=30.0)).track, video)
    result = benchmark.time_pipeline(video, StubPalm(), StubLandmarks([0.99]))
    assert result.frames > 30 and result.palm_runs == 1
    assert result.decode_ms_per_frame > 0 and result.landmark_ms_per_frame > 0
    assert result.total_ms_per_frame >= result.decode_ms_per_frame


def test_missing_models_are_reported_not_raised(tmp_path: Path) -> None:
    video = tmp_path / "c.mp4"
    render_video(generate_tapping(TapSpec(taps=6, fps=30.0)).track, video)
    assert (
        benchmark.main([str(video), "--models", str(tmp_path), "--out", str(tmp_path / "o")]) == 1
    )


def test_peak_memory_is_a_plausible_number_or_unknown() -> None:
    memory = benchmark.peak_memory_mb()
    assert memory is None or 10 < memory < 100_000
