import numpy as np
import pytest
from hypothesis import given
from hypothesis import strategies as st

from axonhand.metrics import count_peaks, normalise_by_palm


def test_tapping_count_recovered_from_synthetic_signal() -> None:
    fps, hz, secs = 30, 2.0, 10
    t = np.arange(fps * secs) / fps
    sig = 0.5 + 0.5 * np.sin(2 * np.pi * hz * t - np.pi / 2 + 0.3)
    assert abs(count_peaks(sig, 0.9) - int(hz * secs)) <= 1


def test_bad_palm_length() -> None:
    with pytest.raises(ValueError):
        normalise_by_palm(np.ones(3), 0.0)


@given(st.floats(min_value=0.01, max_value=10.0))
def test_normalisation_is_scale_invariant(scale: float) -> None:
    d = np.array([1.0, 2.0, 3.0])
    a = normalise_by_palm(d, 1.0)
    b = normalise_by_palm(d * scale, scale)
    assert np.allclose(a, b)
