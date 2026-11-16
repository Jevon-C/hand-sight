"""Comparing two sets of landmarks."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
import pytest
from numpy.typing import NDArray

from axonhand.landmarks import LandmarkTrack
from axonhand.tools import compare_landmarks
from axonhand.tools.compare_landmarks import compare, load, main


def _hand(shift: float = 0.0) -> np.ndarray:
    points = np.zeros((21, 3))
    points[:, 0] = np.arange(21) * 10.0 + shift
    points[9] = [0.0 + shift, -100.0, 0.0]  # palm length 100 from the wrist at the origin
    points[0] = [shift, 0.0, 0.0]
    return points


def test_identical_landmarks_have_no_error() -> None:
    d = compare([_hand(), None], [_hand(), None])
    assert (d.both_found, d.disagree, d.maximum) == (1, 0, 0.0)


def test_error_is_in_palm_lengths_and_names_the_worst_point() -> None:
    other = _hand()
    other[5, 0] += 50.0
    d = compare([_hand()], [other])
    assert d.maximum == pytest.approx(0.5)
    assert d.worst_landmark == 5
    assert d.worst_frame == 0


def test_frames_where_only_one_finds_a_hand_are_counted() -> None:
    d = compare([_hand(), _hand()], [_hand(), None])
    assert (d.both_found, d.disagree) == (1, 1)


def test_different_lengths_and_no_overlap_are_errors() -> None:
    with pytest.raises(ValueError, match="different"):
        compare([_hand()], [_hand(), _hand()])
    with pytest.raises(ValueError, match="no frame"):
        compare([None], [None])


def test_command_line_prints_a_summary(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    a, b = tmp_path / "a.json", tmp_path / "b.json"
    a.write_text(json.dumps([_hand().tolist()] * 3))
    b.write_text(json.dumps([_hand(1.0).tolist()] * 3))
    assert len(load(a)) == 3
    assert main([str(a), str(b)]) == 0
    out = capsys.readouterr().out
    assert "median" in out
    assert "tap analysis not possible" in out


def test_command_line_reports_mismatched_files(tmp_path: Path) -> None:
    a, b = tmp_path / "a.json", tmp_path / "b.json"
    a.write_text(json.dumps([None]))
    b.write_text(json.dumps([None, None]))
    assert main([str(a), str(b)]) == 1


def test_npz_tracks_load_with_missing_frames_as_none_and_compare_by_default_fps(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    hand = np.tile(np.linspace(1.0, 2.0, 21)[:, None], (1, 3))
    points = np.stack([hand, np.full((21, 3), np.nan), hand + 0.01])
    path = tmp_path / "track.npz"
    LandmarkTrack(fps=24.0, points=points).save(path)
    frames = compare_landmarks.load(path)
    assert frames[1] is None and frames[0] is not None
    assert compare_landmarks.main([str(path), str(path)]) == 0
    assert "hand found in both for 2" in capsys.readouterr().out


def test_the_worst_frames_are_listed_worst_first() -> None:
    hand = np.tile(np.linspace(1.0, 2.0, 21)[:, None], (1, 3))
    moved = hand.copy()
    moved[5, 0] += 3.0
    frames: list[NDArray[np.float64] | None] = [hand, hand, hand]
    other: list[NDArray[np.float64] | None] = [hand, moved, hand]
    difference = compare(frames, other)
    assert difference.worst_frames[0][0] == 1
    assert difference.over_limit == 1
    assert difference.first_difference == 1
    assert compare(frames, frames).first_difference is None
