"""The drawing tool that shows what the hand model found."""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

from axonhand.landmarks import LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping, render_video
from axonhand.tools import draw_landmarks


def _clip(tmp_path: Path) -> tuple[Path, LandmarkTrack]:
    synthetic = generate_tapping(TapSpec(taps=4, rate_hz=2.0))
    video = tmp_path / "clip.mp4"
    render_video(synthetic.track, video)
    # Put the synthetic landmarks (palm lengths) into pixel positions on the 320 x 240 frame.
    points = synthetic.track.points.copy()
    points[:, :, 0] = 160 + 80 * points[:, :, 0]
    points[:, :, 1] = 216 - 80 * points[:, :, 1]
    return video, LandmarkTrack(fps=synthetic.track.fps, points=points)


def test_a_sheet_and_a_video_are_written(tmp_path: Path) -> None:
    video, track = _clip(tmp_path)
    track.save(tmp_path / "t.npz")
    code = draw_landmarks.main(
        [
            str(video),
            str(tmp_path / "t.npz"),
            str(tmp_path / "sheet.png"),
            "--video-out",
            str(tmp_path / "drawn.mp4"),
            "--frames",
            "4",
        ]
    )
    assert code == 0
    sheet = Image.open(tmp_path / "sheet.png")
    assert sheet.width == 3 * 480 and sheet.height >= 2 * 360
    assert (tmp_path / "drawn.mp4").stat().st_size > 0


def test_a_frame_without_a_hand_is_marked_and_a_frame_with_one_is_drawn_on() -> None:
    frame = np.full((240, 320, 3), 100, dtype=np.uint8)
    none = np.full((21, 3), np.nan)
    assert (np.asarray(draw_landmarks.draw(frame, none)) != 100).any()  # the note
    hand = np.full((21, 3), np.nan)
    hand[8] = (160, 120, 0)
    drawn = np.asarray(draw_landmarks.draw(frame, hand))
    assert tuple(drawn[120, 160]) == (60, 120, 255)  # the index tip, blue


def test_a_missing_file_is_reported_not_raised(tmp_path: Path) -> None:
    assert draw_landmarks.main([str(tmp_path / "a.mp4"), str(tmp_path / "b.npz"), "x.png"]) == 2
