"""Metric primitives. Each is tested against synthetic motion with known truth."""

from __future__ import annotations

import numpy as np
from numpy.typing import NDArray


def normalise_by_palm(distance: NDArray[np.float64], palm_length: float) -> NDArray[np.float64]:
    """Scale distances by palm length so results are camera-distance independent."""
    if palm_length <= 0 or not np.isfinite(palm_length):
        raise ValueError("palm_length must be positive and finite")
    return np.asarray(distance / palm_length, dtype=np.float64)


def count_peaks(signal: NDArray[np.float64], min_height: float) -> int:
    """Count local maxima above min_height (strict interior maxima)."""
    if signal.size < 3:
        return 0
    mid = signal[1:-1]
    mask = (mid > signal[:-2]) & (mid >= signal[2:]) & (mid > min_height)
    return int(mask.sum())
