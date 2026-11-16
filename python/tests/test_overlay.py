"""Landmarks as data for drawing over the video."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np

from axonhand.landmarks import LANDMARK_COUNT, LandmarkTrack


def _track() -> LandmarkTrack:
    points = np.full((3, LANDMARK_COUNT, 3), np.nan)
    points[0, :, :2] = np.arange(LANDMARK_COUNT)[:, None] + 0.123456  # every landmark found
    points[1, 4, :2] = (10.04, 20.06)  # only one found
    return LandmarkTrack(fps=25.0, points=points)  # frame 2: nothing found


def test_overlay_has_one_entry_per_frame_with_nulls_for_what_is_missing() -> None:
    data = _track().overlay()
    assert data["fps"] == 25.0
    frames = data["points"]
    assert isinstance(frames, list)
    assert len(frames) == 3
    assert frames[2] is None
    assert frames[0] is not None
    assert len(frames[0]) == LANDMARK_COUNT
    assert frames[0][0] == [0.1, 0.1]
    assert frames[1] is not None
    assert frames[1][4] == [10.0, 20.1]
    assert sum(entry is None for entry in frames[1]) == LANDMARK_COUNT - 1


def test_saved_overlay_is_plain_valid_json(tmp_path: Path) -> None:
    path = tmp_path / "landmarks.json"
    _track().save_overlay(path)
    text = path.read_text(encoding="utf-8")
    assert "NaN" not in text
    assert json.loads(text)["fps"] == 25.0
