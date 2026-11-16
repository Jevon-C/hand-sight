"""Opening and closing, turning and tremor, each checked against synthetic truth."""

from __future__ import annotations

import numpy as np
import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from axonhand.landmarks import LANDMARK_COUNT, LandmarkTrack
from axonhand.movement import MIN_CYCLES, analyse_open_close, openness_signal
from axonhand.pronation import MIN_TURNS, analyse_turns, face_signal
from axonhand.synthetic_movements import (
    OpenCloseSpec,
    TremorSpec,
    TurnSpec,
    generate_open_close,
    generate_tremor,
    generate_turns,
)
from axonhand.tapping import AnalysisError
from axonhand.tremor import MIN_SECONDS, TremorParameters, analyse_tremor, welch


def _blank(frames: int = 600, fps: float = 60.0) -> LandmarkTrack:
    return LandmarkTrack(fps=fps, points=np.full((frames, LANDMARK_COUNT, 3), np.nan))


# ----- opening and closing ---------------------------------------------------------------------


def test_open_close_measures_match_the_truth() -> None:
    track, truth = generate_open_close(OpenCloseSpec(rate_hz=2.0, cycles=24, decrement=0.3))
    result = analyse_open_close(track)
    assert result.cycle_count == 24
    assert result.cycle_rate_hz == pytest.approx(2.0, rel=0.02)
    assert result.mean_range == pytest.approx(truth.ranges.mean(), rel=0.03)
    assert result.range_decrement_percent == pytest.approx(30.0, abs=3.0)
    assert (result.hesitations, result.arrests) == (0, 0)
    assert np.allclose(result.peak_times, truth.peak_times, atol=1.5 / track.fps)


def test_open_close_survives_tracker_noise() -> None:
    track, _ = generate_open_close(OpenCloseSpec(cycles=20, noise=0.01, seed=3))
    result = analyse_open_close(track)
    assert result.cycle_count == 20
    assert result.cycle_rate_hz == pytest.approx(2.0, rel=0.03)


def test_a_hand_held_open_at_the_start_does_not_shrink_the_first_cycle() -> None:
    track, truth = generate_open_close(OpenCloseSpec(cycles=12))
    result = analyse_open_close(track)
    assert result.ranges[0] == pytest.approx(truth.ranges[0], rel=0.05)
    assert result.ranges[-1] == pytest.approx(truth.ranges[-1], rel=0.05)


def test_open_close_with_too_few_cycles_is_an_error() -> None:
    track, _ = generate_open_close(OpenCloseSpec(cycles=3))
    with pytest.raises(AnalysisError, match=str(MIN_CYCLES)):
        analyse_open_close(track)


def test_a_still_hand_has_no_cycles() -> None:
    track, _ = generate_open_close(OpenCloseSpec(cycles=6, open_distance=1.0))
    with pytest.raises(AnalysisError):
        analyse_open_close(track)


def test_open_close_with_no_hand_is_an_error() -> None:
    with pytest.raises(AnalysisError):
        openness_signal(_blank())


def test_openness_signal_fills_gaps_in_the_fingertips() -> None:
    track, _ = generate_open_close(OpenCloseSpec(cycles=8))
    points = track.points.copy()
    points[100:104, 8:21, :] = np.nan  # several fingertips lost for four frames
    signal, missing = openness_signal(LandmarkTrack(fps=track.fps, points=points))
    assert missing > 0
    assert np.isfinite(signal).all()


# ----- turning ---------------------------------------------------------------------------------


def test_turns_match_the_truth() -> None:
    track, truth = generate_turns(TurnSpec(rate_hz=1.5, cycles=12))
    result = analyse_turns(track)
    assert result.turn_count == truth.turn_times.size
    assert result.cycle_rate_hz == pytest.approx(1.5, rel=0.02)
    assert np.allclose(result.turn_times, truth.turn_times, atol=1.5 / track.fps)
    assert (result.hesitations, result.arrests) == (0, 0)


def test_turn_size_decrement_is_positive_when_the_swing_shrinks() -> None:
    steady = analyse_turns(generate_turns(TurnSpec(cycles=14))[0])
    shrinking = analyse_turns(generate_turns(TurnSpec(cycles=14, decrement=0.4))[0])
    assert abs(steady.excursion_decrement_percent) < 2.0
    assert shrinking.excursion_decrement_percent > 5.0


def test_turns_do_not_depend_on_which_face_is_which() -> None:
    track, _ = generate_turns(TurnSpec(cycles=10))
    mirrored = track.points.copy()
    mirrored[:, :, 0] *= -1.0
    result = analyse_turns(LandmarkTrack(fps=track.fps, points=mirrored))
    assert result.turn_count == analyse_turns(track).turn_count


def test_turns_survive_noise() -> None:
    track, truth = generate_turns(TurnSpec(cycles=12, noise=0.01, seed=5))
    assert analyse_turns(track).turn_count == truth.turn_times.size


def test_a_hand_that_never_turns_cannot_be_measured() -> None:
    track, _ = generate_turns(TurnSpec(cycles=8, swing_degrees=0.0))
    with pytest.raises(AnalysisError, match="could not be told apart"):
        analyse_turns(track)


def test_too_few_turns_is_an_error() -> None:
    track, _ = generate_turns(TurnSpec(cycles=2))
    with pytest.raises(AnalysisError, match=str(MIN_TURNS)):
        analyse_turns(track)


def test_turning_with_no_hand_is_an_error() -> None:
    with pytest.raises(AnalysisError):
        face_signal(_blank())


# ----- tremor ----------------------------------------------------------------------------------


def test_tremor_frequency_and_size_match_the_truth() -> None:
    result = analyse_tremor(generate_tremor(TremorSpec(frequency_hz=5.0, rms=0.02)))
    assert result.dominant_frequency_hz == pytest.approx(5.0, abs=0.1)
    assert result.amplitude_rms == pytest.approx(0.02, rel=0.05)
    assert result.amplitude_peak_to_peak == pytest.approx(0.02 * 2 * 2**0.5, rel=0.05)
    assert result.regularity > 0.9
    assert result.band_share_percent > 80


@settings(max_examples=15, deadline=None)
@given(frequency=st.floats(3.5, 11.0), rms=st.floats(0.01, 0.05))
def test_tremor_is_found_wherever_it_is(frequency: float, rms: float) -> None:
    result = analyse_tremor(generate_tremor(TremorSpec(frequency_hz=frequency, rms=rms)))
    assert result.dominant_frequency_hz == pytest.approx(frequency, abs=0.15)
    assert result.amplitude_rms == pytest.approx(rms, rel=0.08)


def test_a_slow_drift_is_not_counted_as_tremor() -> None:
    drift_only = analyse_tremor(generate_tremor(TremorSpec(rms=1e-6, drift=0.1)))
    assert drift_only.amplitude_rms < 0.002


def test_noise_floor_is_estimated_and_the_tremor_is_above_it() -> None:
    result = analyse_tremor(generate_tremor(TremorSpec(rms=0.02, noise=0.002)))
    assert result.noise_rms is not None
    assert result.amplitude_rms > 3 * result.noise_rms


def test_a_tremor_as_small_as_the_noise_is_not_clearly_above_it() -> None:
    result = analyse_tremor(generate_tremor(TremorSpec(rms=0.0005, noise=0.005)))
    assert result.noise_rms is not None
    assert result.amplitude_rms < 3 * result.noise_rms


def test_tremor_at_thirty_frames_per_second_is_still_found() -> None:
    result = analyse_tremor(generate_tremor(TremorSpec(frequency_hz=6.0, fps=30.0)))
    assert result.dominant_frequency_hz == pytest.approx(6.0, abs=0.15)


def test_a_short_video_is_an_error() -> None:
    track = generate_tremor(TremorSpec(seconds=MIN_SECONDS - 1))
    with pytest.raises(AnalysisError, match="at least"):
        analyse_tremor(track)


def test_a_very_low_frame_rate_is_an_error() -> None:
    with pytest.raises(AnalysisError, match="frame rate"):
        analyse_tremor(generate_tremor(TremorSpec(fps=8.0, seconds=20.0)))


def test_tremor_with_no_hand_is_an_error() -> None:
    with pytest.raises(AnalysisError):
        analyse_tremor(_blank())


def test_lost_frames_are_interpolated_over() -> None:
    track = generate_tremor(TremorSpec(frequency_hz=5.0, rms=0.02))
    points = track.points.copy()
    points[200:205] = np.nan
    result = analyse_tremor(LandmarkTrack(fps=track.fps, points=points))
    assert result.missing_fraction > 0
    assert result.dominant_frequency_hz == pytest.approx(5.0, abs=0.15)


def test_tremor_parameters_are_checked() -> None:
    with pytest.raises(ValueError, match="band_low"):
        TremorParameters(band_low=8.0, band_high=4.0)
    with pytest.raises(ValueError, match="window"):
        TremorParameters(window_seconds=0.2)


def test_welch_puts_a_pure_tone_at_its_frequency_with_its_power() -> None:
    fps = 100.0
    t = np.arange(2000) / fps
    freqs, power = welch(np.sin(2 * np.pi * 7.0 * t), fps, 2.0)
    assert freqs[np.argmax(power)] == pytest.approx(7.0, abs=0.3)
    assert power.sum() * (freqs[1] - freqs[0]) == pytest.approx(0.5, rel=0.1)
