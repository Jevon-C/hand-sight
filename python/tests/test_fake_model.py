"""The whole path video -> landmarks -> measures, against known truth."""

import json
from pathlib import Path

import av
import numpy as np
import pytest

from axonhand import fake_model
from axonhand.landmarks import LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping, render_video
from axonhand.tapping import analyse


def test_measures_survive_drawing_compressing_and_tracking(tmp_path: Path) -> None:
    spec = TapSpec(rate_hz=3.0, taps=24, fps=60.0, amplitude=0.8, decrement=0.3, pauses=[(10, 0.5)])
    data = generate_tapping(spec)
    video = tmp_path / "taps.mp4"
    render_video(data.track, video)

    track = fake_model.track_markers(video)
    assert track.frames == data.track.frames
    assert track.fps == pytest.approx(60.0)

    result = analyse(track)
    assert result.tap_count == 24
    assert result.tap_rate_hz == pytest.approx(data.truth.rate_hz, rel=0.03)
    assert result.mean_amplitude == pytest.approx(float(data.truth.amplitudes.mean()), rel=0.06)
    assert result.amplitude_decrement_percent == pytest.approx(30.0, abs=6.0)
    assert result.hesitations == 1
    assert np.abs(result.peak_times - data.truth.peak_times).max() <= 2.0 / 60.0


def test_an_unreadable_file_is_a_tracking_error(tmp_path: Path) -> None:
    junk = tmp_path / "junk.mp4"
    junk.write_text("not a video")
    with pytest.raises(fake_model.TrackingError):
        fake_model.track_markers(junk)


def test_command_line_writes_a_track_and_reports_failure(tmp_path: Path) -> None:
    video = tmp_path / "taps.mp4"
    render_video(generate_tapping(TapSpec(taps=6, fps=30.0)).track, video)
    out = tmp_path / "track.npz"
    assert fake_model.main([str(video), str(out)]) == 0
    assert LandmarkTrack.load(out).frames > 0
    assert out.with_suffix(".json").exists()
    assert fake_model.main([str(tmp_path / "absent.mp4"), str(out)]) == 2


def test_the_overlay_puts_the_dots_where_they_are_drawn(tmp_path: Path) -> None:
    video = tmp_path / "taps.mp4"
    render_video(generate_tapping(TapSpec(taps=6, fps=30.0)).track, video)
    out = tmp_path / "track.npz"
    assert fake_model.main([str(video), str(out)]) == 0
    overlay = json.loads(out.with_suffix(".json").read_text(encoding="utf-8"))
    assert overlay["fps"] == pytest.approx(30.0)
    track = LandmarkTrack.load(out)
    assert len(overlay["points"]) == track.frames
    with av.open(str(video)) as container:
        stream = container.streams.video[0]
        width, height = int(stream.width), int(stream.height)
    seen = [p for frame in overlay["points"] if frame for p in frame if p is not None]
    assert seen
    assert all(0 <= x <= width and 0 <= y <= height for x, y in seen)
