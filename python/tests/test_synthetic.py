"""Tests for the synthetic generator itself: it must produce what it says it produces."""

from pathlib import Path

import numpy as np
import pytest

from axonhand.landmarks import INDEX_TIP, THUMB_TIP, LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping


def tip_distance(track: LandmarkTrack) -> np.ndarray:
    return np.linalg.norm(track.points[:, THUMB_TIP, :] - track.points[:, INDEX_TIP, :], axis=1)


def test_peaks_sit_where_the_truth_says() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.0, taps=10, fps=120.0, amplitude=0.9))
    distance = tip_distance(data.track)
    for time, amplitude in zip(data.truth.peak_times, data.truth.amplitudes, strict=True):
        index = int(round(time * data.track.fps))
        assert distance[index] == pytest.approx(amplitude, abs=1e-3)


def test_fingers_touch_between_taps() -> None:
    data = generate_tapping(TapSpec(rate_hz=3.0, taps=5))
    assert tip_distance(data.track).min() == pytest.approx(0.0, abs=1e-9)


def test_amplitude_falls_linearly_by_the_decrement() -> None:
    truth = generate_tapping(TapSpec(taps=11, amplitude=1.0, decrement=0.4)).truth
    assert truth.amplitudes[0] == pytest.approx(1.0)
    assert truth.amplitudes[-1] == pytest.approx(0.6)
    assert np.allclose(np.diff(truth.amplitudes), -0.04)


def test_a_pause_lengthens_exactly_one_interval() -> None:
    truth = generate_tapping(TapSpec(rate_hz=2.0, taps=8, pauses=[(3, 1.0)])).truth
    assert truth.intervals[3] == pytest.approx(1.5)
    assert np.allclose(np.delete(truth.intervals, 3), 0.5)


def test_truth_rate_counts_intervals() -> None:
    truth = generate_tapping(TapSpec(rate_hz=4.0, taps=21)).truth
    assert truth.rate_hz == pytest.approx(4.0)
    assert truth.tap_count == 21


def test_noise_is_seeded_and_reproducible() -> None:
    a = generate_tapping(TapSpec(noise=0.01, seed=3)).track.points
    b = generate_tapping(TapSpec(noise=0.01, seed=3)).track.points
    c = generate_tapping(TapSpec(noise=0.01, seed=4)).track.points
    assert np.array_equal(a, b, equal_nan=True)
    assert not np.array_equal(a, c, equal_nan=True)


@pytest.mark.parametrize(
    "bad",
    [
        {"rate_hz": 0.0},
        {"taps": 1},
        {"decrement": 1.0},
        {"noise": -1.0},
        {"pauses": [(99, 1.0)]},
        {"pauses": [(0, 0.0)]},
    ],
)
def test_invalid_specs_are_refused(bad: dict[str, object]) -> None:
    with pytest.raises(ValueError):
        TapSpec(**bad)  # type: ignore[arg-type]


def test_track_round_trips_through_a_file(tmp_path: Path) -> None:
    track = generate_tapping(TapSpec(taps=4)).track
    track.save(tmp_path / "t.npz")
    back = LandmarkTrack.load(tmp_path / "t.npz")
    assert back.fps == track.fps
    assert np.array_equal(back.points, track.points, equal_nan=True)
    (tmp_path / "bad.npz").write_bytes(b"nope")
    with pytest.raises(ValueError):
        LandmarkTrack.load(tmp_path / "bad.npz")


def test_a_track_must_have_the_landmark_shape() -> None:
    with pytest.raises(ValueError):
        LandmarkTrack(fps=30.0, points=np.zeros((5, 3, 3)))
    with pytest.raises(ValueError):
        LandmarkTrack(fps=0.0, points=np.zeros((5, 21, 3)))
