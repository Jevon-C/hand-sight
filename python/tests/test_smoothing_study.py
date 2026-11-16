"""The smoothing study and the smoothers it compares."""

import numpy as np
import pytest

from axonhand.tapping import _smooth
from axonhand.tools import smoothing_study as study


def test_every_smoother_keeps_the_length_and_the_mean_of_a_noisy_signal() -> None:
    rng = np.random.default_rng(0)
    clean = 0.4 + 0.3 * np.sin(np.linspace(0, 12, 360))
    noisy = clean + rng.normal(0, 0.03, clean.size)
    for smoother in (_smooth, study.kalman(60.0, 30.0, 0.03), study.identity):
        out = smoother(noisy)
        assert out.shape == noisy.shape
        assert abs(out.mean() - clean.mean()) < 0.02
    assert (
        np.abs(study.kalman(60.0, 30.0, 0.03)(noisy) - clean).mean() < np.abs(noisy - clean).mean()
    )


def test_the_kalman_smoother_passes_a_missing_value_through_as_a_gap_it_fills() -> None:
    signal = 0.5 + 0.2 * np.sin(np.linspace(0, 6, 120))
    signal[40:43] = np.nan
    out = study.kalman(30.0, 20.0, 0.01)(signal)
    assert np.isfinite(out).all()


def test_the_study_runs_every_smoother_and_loses_nothing_without_noise() -> None:
    rows = study.study(0.0, fps_list=(30.0,))
    assert [r.smoother for r in rows][:2] == ["none", "3-point average"]
    assert all(r.failures == 0 and r.count_errors == 0 for r in rows)
    text = study.markdown(rows)
    assert "Smoothing study" in text and text.count("| 0.00 |") == len(rows)
    assert rows[0].peak_time_ms == pytest.approx(rows[1].peak_time_ms, abs=5.0)
