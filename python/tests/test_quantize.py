"""The quantisation tool: its pieces are checked with the real model on synthetic inputs."""

from pathlib import Path

import numpy as np
import pytest

from axonhand.hand.runtime import open_model
from axonhand.landmarks import LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping
from axonhand.tools import quantize

MODELS = Path(__file__).resolve().parents[2] / "models"
LANDMARK = MODELS / "handpose_estimation_mediapipe_2023feb.onnx"
needs_models = pytest.mark.skipif(not LANDMARK.is_file(), reason="models not present")


def _blobs(count: int) -> list[np.ndarray]:
    rng = np.random.default_rng(0)
    return [rng.uniform(0.2, 0.8, size=(1, 224, 224, 3)).astype(np.float32) for _ in range(count)]


def test_the_inputs_are_split_into_calibration_and_held_out_halves() -> None:
    blobs = _blobs(10)
    calibration, held_out = quantize.split_blobs(blobs, limit=6)
    assert len(calibration) == 3 and len(held_out) == 3
    with pytest.raises(ValueError):
        quantize.split_blobs(blobs[:3], limit=6)


def test_recording_keeps_a_copy_of_every_input() -> None:
    class Echo:
        def run(self, blob: np.ndarray) -> list[np.ndarray]:
            return [blob]

    recorder = quantize.RecordingModel(Echo())
    data = np.ones((1, 2), dtype=np.float32)
    recorder.run(data)
    data[:] = 5
    assert recorder.inputs[0].sum() == 2


def test_the_landmark_error_of_a_model_against_itself_is_zero() -> None:
    class Fixed:
        def run(self, blob: np.ndarray) -> list[np.ndarray]:
            points = np.arange(63, dtype=np.float32).reshape(1, 63) * 3.0
            return [points, np.array([[0.9]], dtype=np.float32)]

    error = quantize.landmark_error(Fixed(), Fixed(), _blobs(3))
    assert error.median == 0 and error.worst == 0 and error.score_difference == 0


def test_the_landmark_error_is_measured_in_palm_lengths() -> None:
    class Hand:
        def __init__(self, shift: float) -> None:
            self.shift = shift

        def run(self, blob: np.ndarray) -> list[np.ndarray]:
            points = np.zeros((21, 3), dtype=np.float32)
            points[9, 1] = 100.0  # the palm is 100 pixels long
            points[:, 0] += self.shift
            return [points.reshape(1, 63), np.array([[0.9]], dtype=np.float32)]

    error = quantize.landmark_error(Hand(0.0), Hand(2.0), _blobs(2))
    assert error.median == pytest.approx(0.02, abs=1e-6)


def test_tap_comparison_of_identical_tracks_shows_no_difference() -> None:
    track = generate_tapping(TapSpec(taps=12, fps=30.0)).track
    result = quantize.compare_tracks(track, LandmarkTrack(30.0, track.points.copy()))
    assert result.float_taps == result.quantised_taps
    assert result.contact_diff_ms_max == 0 and result.rate_diff_percent == 0


def test_the_verdict_names_each_limit() -> None:
    good = quantize.LandmarkError(0.01, 0.03, 0.1, 0.01)
    taps = quantize.TapComparison(15, 15, 20.0, 0.0)
    assert all(ok for _, ok in quantize.verdict(good, taps, fps=30.0))
    bad = quantize.verdict(quantize.LandmarkError(0.05, 0.2, 0.5, 0.1), taps, fps=30.0)
    assert [ok for _, ok in bad][:2] == [False, False]
    late = quantize.verdict(good, quantize.TapComparison(15, 14, 80.0, 0.0), fps=30.0)
    assert [ok for _, ok in late][2:] == [False, False]


@needs_models
@pytest.mark.parametrize("activation", ["uint8", "uint16"])
def test_a_quantised_model_loads_runs_and_keeps_its_output_shape(
    tmp_path: Path, activation: str
) -> None:
    floating = open_model(LANDMARK, "cpu")
    blobs = _blobs(6)
    target = tmp_path / "q.onnx"
    quantize.quantise(LANDMARK, target, floating.input_name, blobs[:3], activation)
    quantised = open_model(target, "cpu")
    out = quantised.run(blobs[4])
    assert out[0].shape == floating.run(blobs[4])[0].shape
    assert target.stat().st_size < LANDMARK.stat().st_size
    assert np.isfinite(out[0]).all()
