"""The finger tapping analysis is checked against synthetic taps with known truth."""

import numpy as np
import pytest

from axonhand.landmarks import INDEX_TIP, LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping
from axonhand.tapping import (
    AnalysisError,
    TappingParameters,
    analyse,
    find_peaks,
    opening_signal,
)


@pytest.mark.parametrize("rate", [1.0, 2.0, 3.5, 5.0])
@pytest.mark.parametrize("fps", [30.0, 60.0, 120.0])
def test_count_rate_and_size_are_recovered(rate: float, fps: float) -> None:
    data = generate_tapping(
        TapSpec(rate_hz=rate, taps=max(12, int(rate * 8)), fps=fps, amplitude=0.7)
    )
    result = analyse(data.track)
    assert result.tap_count == data.truth.tap_count
    assert result.tap_rate_hz == pytest.approx(data.truth.rate_hz, rel=0.02)
    assert result.mean_amplitude == pytest.approx(0.7, rel=0.05)
    assert result.hesitations == 0 and result.arrests == 0


@pytest.mark.parametrize("decrement", [0.0, 0.2, 0.5])
def test_the_decrement_is_recovered(decrement: float) -> None:
    result = analyse(generate_tapping(TapSpec(taps=30, amplitude=0.8, decrement=decrement)).track)
    assert result.amplitude_decrement_percent == pytest.approx(100 * decrement, abs=3.0)


def test_tap_times_match_the_truth() -> None:
    data = generate_tapping(TapSpec(rate_hz=3.0, taps=20, fps=60.0))
    result = analyse(data.track)
    assert np.abs(result.peak_times - data.truth.peak_times).max() <= 1.5 / 60.0


def test_an_amplitude_that_does_not_change_has_no_variability() -> None:
    result = analyse(generate_tapping(TapSpec(taps=20, decrement=0.0)).track)
    assert result.amplitude_variability_percent < 3.0


def test_a_hesitation_and_an_arrest_are_told_apart() -> None:
    # Period 0.4 s. A 0.4 s extra pause makes an interval of 2x typical (hesitation); a 1.0 s
    # pause makes 3.5x (arrest).
    spec = TapSpec(rate_hz=2.5, taps=24, pauses=[(5, 0.4), (15, 1.0)])
    result = analyse(generate_tapping(spec).track)
    assert result.hesitations == 1
    assert result.arrests == 1
    assert result.tap_count == 24


def test_a_slower_hand_gives_a_lower_rate() -> None:
    fast = analyse(generate_tapping(TapSpec(rate_hz=4.0, taps=32)).track).tap_rate_hz
    slow = analyse(generate_tapping(TapSpec(rate_hz=2.0, taps=16)).track).tap_rate_hz
    assert fast > 1.8 * slow


def test_small_noise_does_not_change_the_answer() -> None:
    data = generate_tapping(TapSpec(rate_hz=3.0, taps=30, noise=0.01, seed=7))
    result = analyse(data.track)
    assert result.tap_count == 30
    assert result.tap_rate_hz == pytest.approx(3.0, rel=0.03)


def test_distance_from_the_camera_does_not_matter() -> None:
    data = generate_tapping(TapSpec(taps=20))
    scaled = LandmarkTrack(fps=data.track.fps, points=data.track.points * 7.5)
    a, b = analyse(data.track), analyse(scaled)
    assert a.mean_amplitude == pytest.approx(b.mean_amplitude)
    assert a.tap_count == b.tap_count


def test_missing_frames_are_bridged_and_counted() -> None:
    data = generate_tapping(TapSpec(rate_hz=3.0, taps=24, fps=60.0))
    points = data.track.points.copy()
    points[100:104, INDEX_TIP, :] = np.nan
    result = analyse(LandmarkTrack(fps=60.0, points=points))
    assert result.tap_count == 24
    assert result.missing_fraction == pytest.approx(4 / points.shape[0])


def test_failure_messages_are_clear() -> None:
    flat = generate_tapping(TapSpec(taps=10)).track
    points = flat.points.copy()
    points[:, 4, :] = points[:, 8, :]  # thumb never leaves the index
    with pytest.raises(AnalysisError, match="found"):
        analyse(LandmarkTrack(fps=60.0, points=points))
    gone = flat.points.copy()
    gone[:, INDEX_TIP, :] = np.nan
    with pytest.raises(AnalysisError, match="half"):
        analyse(LandmarkTrack(fps=60.0, points=gone))
    nowrist = flat.points.copy()
    nowrist[:, 0, :] = np.nan
    with pytest.raises(AnalysisError, match="hand size"):
        analyse(LandmarkTrack(fps=60.0, points=nowrist))


def test_parameters_are_validated() -> None:
    with pytest.raises(ValueError):
        TappingParameters(min_amplitude=0.0)
    with pytest.raises(ValueError):
        TappingParameters(hesitation_factor=3.0, arrest_factor=2.0)


def test_a_larger_minimum_ignores_small_taps() -> None:
    data = generate_tapping(TapSpec(taps=20, amplitude=0.2))
    with pytest.raises(AnalysisError):
        analyse(data.track, TappingParameters(min_amplitude=0.5))


def test_peak_finder_keeps_the_higher_of_two_close_maxima() -> None:
    fps = 60.0
    t = np.arange(600) / fps
    signal = 0.5 * np.sin(np.pi * 3.0 * t) ** 2
    peaks = find_peaks(signal, fps, 0.1)
    assert np.all(np.diff(peaks) >= 0.5 * fps / 3.0 * 0.9)


def test_signal_has_the_right_length() -> None:
    track = generate_tapping(TapSpec(taps=5)).track
    signal, missing = opening_signal(track)
    assert signal.shape == (track.frames,) and missing == 0.0


@pytest.mark.parametrize("rate", [1.0, 2.5, 4.0])
def test_contact_comes_after_each_peak_and_before_the_next(rate: float) -> None:
    data = generate_tapping(TapSpec(rate_hz=rate, taps=15, fps=60.0, pauses=((4, 0.6),)))
    result = analyse(data.track)
    assert result.contact_times.size == result.tap_count
    lag = result.contact_times - result.peak_times
    period = 1.0 / rate
    assert np.all(lag > 0.35 * period) and np.all(lag <= 0.5 * period + 1 / 60.0)
    assert np.all(result.contact_times[:-1] < result.peak_times[1:])


def test_a_pause_with_the_fingers_closed_does_not_move_the_contact() -> None:
    with_pause = analyse(generate_tapping(TapSpec(rate_hz=2.0, taps=12, pauses=((5, 1.0),))).track)
    lag = with_pause.contact_times - with_pause.peak_times
    assert lag.max() - lag.min() < 0.05


def test_a_closing_that_was_not_filmed_gives_no_last_contact() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.0, taps=12, fps=60.0))
    last_peak = int(round(data.truth.peak_times[-1] * 60.0))
    cut = LandmarkTrack(60.0, data.track.points[: last_peak + 2])
    result = analyse(cut)
    assert result.contact_times.size == result.tap_count - 1


def _with_held_open_start(seconds: float, level: float, wobble: float) -> tuple[LandmarkTrack, int]:
    """A tapping track preceded by the hand held open and wobbling slightly, and its tap count."""
    from axonhand.landmarks import THUMB_TIP

    data = generate_tapping(TapSpec(rate_hz=2.0, taps=12, fps=30.0, lead_in=0.0))
    frames = int(seconds * 30.0)
    hold = np.repeat(data.track.points[:1], frames, axis=0)
    direction = np.array([-0.8, -0.6, 0.0])
    phase = np.arange(frames) * 0.9
    distance = level + wobble * np.sin(phase) + wobble * np.sin(2.3 * phase + 1.0)
    hold[:, THUMB_TIP, :] = hold[:, INDEX_TIP, :] + distance[:, None] * direction
    return LandmarkTrack(30.0, np.concatenate([hold, data.track.points])), data.truth.tap_count


@pytest.mark.parametrize("level", [0.2, 0.35, 0.5])
def test_a_hand_held_open_before_tapping_adds_no_taps_but_its_closing_is_a_contact(
    level: float,
) -> None:
    track, taps = _with_held_open_start(1.0, level, wobble=0.015)
    result = analyse(track)
    assert result.tap_count == taps
    assert result.contact_times.size == taps + 1
    assert result.contact_times[0] == pytest.approx(1.0, abs=0.2)
