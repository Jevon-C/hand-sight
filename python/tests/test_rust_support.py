"""The two helpers the Rust hand tracker relies on."""

import io
import json
from pathlib import Path

import numpy as np
import pytest

from axonhand.hand.video import TrackingError, read_frames
from axonhand.synthetic import TapSpec, generate_tapping, render_video
from axonhand.tools import decode_frames, ort_paths


def _video(tmp_path: Path) -> Path:
    path = tmp_path / "taps.mp4"
    render_video(generate_tapping(TapSpec(taps=4, fps=30.0)).track, path)
    return path


def test_the_stream_is_a_header_line_and_every_frame_in_full(tmp_path: Path) -> None:
    video = _video(tmp_path)
    out = io.BytesIO()
    count = decode_frames.decode(video, out)
    data = out.getvalue()
    header, _, body = data.partition(b"\n")
    magic, version, width, height, fps = header.decode("ascii").split()
    assert (magic, int(version)) == ("AXONFRAMES", 1)
    assert float(fps) == pytest.approx(30.0)
    assert len(body) == count * int(width) * int(height) * 3
    first = np.frombuffer(body[: int(width) * int(height) * 3], dtype=np.uint8)
    _, frames = read_frames(video)
    expected = next(iter(frames))
    assert expected.shape == (int(height), int(width), 3)
    assert np.array_equal(first.reshape(expected.shape), expected)


def test_a_file_that_is_not_a_video_exits_with_status_two_and_a_message(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    junk = tmp_path / "junk.mp4"
    junk.write_text("not a video")
    with pytest.raises(TrackingError):
        decode_frames.decode(junk, io.BytesIO())
    assert decode_frames.main([str(junk)]) == 2
    assert "axonhand-message: " in capsys.readouterr().err


def test_the_paths_are_plain_data_and_the_runtime_library_exists() -> None:
    described = ort_paths.describe()
    assert set(described) == {"library", "npu_name", "npu_library"}
    json.dumps(described)
    library = described["library"]
    assert library is not None
    assert Path(library).is_file()


def test_asking_for_an_npu_that_is_not_there_exits_with_status_two_and_a_message(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    from axonhand import hand_model

    for name in (hand_model.PALM_MODEL, hand_model.LANDMARK_MODEL):
        (tmp_path / name).write_bytes(b"not a model")
    video = _video(tmp_path)
    code = hand_model.main(
        [str(video), str(tmp_path / "out.npz"), "--models", str(tmp_path), "--device", "npu"]
    )
    assert code == 2
    assert "axonhand-message: The NPU could not be used" in capsys.readouterr().err
