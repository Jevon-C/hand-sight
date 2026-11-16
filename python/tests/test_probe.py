"""Tests for the video probe, using short clips generated on the fly."""

import json
from fractions import Fraction
from pathlib import Path

import av
import numpy as np
import pytest

from axonhand.tools import probe


def make_clip(
    path: Path, *, fps: int = 60, seconds: float = 1.0, size: tuple[int, int] = (64, 48)
) -> None:
    width, height = size
    with av.open(str(path), mode="w") as container:
        stream = container.add_stream("libx264", rate=fps)
        stream.width, stream.height = width, height
        stream.pix_fmt = "yuv420p"
        for index in range(round(fps * seconds)):
            image = np.full((height, width, 3), index % 255, dtype=np.uint8)
            frame = av.VideoFrame.from_ndarray(image, format="rgb24")
            for packet in stream.encode(frame):
                container.mux(packet)
        for packet in stream.encode():
            container.mux(packet)


def test_probe_reads_size_rate_and_length(tmp_path: Path) -> None:
    clip = tmp_path / "clip.mp4"
    make_clip(clip, fps=60, seconds=1.0, size=(64, 48))
    result = probe.probe(clip)
    assert (result.width, result.height) == (64, 48)
    assert Fraction(result.fps).limit_denominator(1000) == 60
    assert result.duration_seconds == pytest.approx(1.0, abs=0.1)
    assert result.frame_count == 60
    assert result.codec == "h264"


def test_missing_file_is_a_probe_error(tmp_path: Path) -> None:
    with pytest.raises(probe.ProbeError, match="no such file"):
        probe.probe(tmp_path / "absent.mp4")


def test_a_file_that_is_not_a_video_is_a_probe_error(tmp_path: Path) -> None:
    junk = tmp_path / "notes.mp4"
    junk.write_text("this is not a video")
    with pytest.raises(probe.ProbeError, match="not a readable video"):
        probe.probe(junk)


def test_command_line_prints_json_and_exit_codes(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    clip = tmp_path / "clip.mp4"
    make_clip(clip, fps=30, seconds=0.5)
    assert probe.main([str(clip)]) == 0
    assert json.loads(capsys.readouterr().out)["width"] == 64
    assert probe.main([str(tmp_path / "absent.mp4")]) == 2
    assert "error" in json.loads(capsys.readouterr().out)
