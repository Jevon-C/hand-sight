"""The evaluation compares found taps with marked taps; checked against known answers."""

import json
import threading
import urllib.error
import urllib.request
from pathlib import Path

import numpy as np
import pytest
from hypothesis import given
from hypothesis import strategies as st

from axonhand.evaluation import tapmarks
from axonhand.evaluation.report import markdown, summarise, to_json, verdicts
from axonhand.evaluation.scoring import (
    ClipScore,
    Failure,
    match_taps,
    score_clip,
    tolerance_seconds,
)
from axonhand.evaluation.tapmarks import AnnotationError, TapAnnotation
from axonhand.landmarks import LandmarkTrack
from axonhand.synthetic import TapSpec, generate_tapping, render_video
from axonhand.tapping import TappingParameters, analyse, gap_counts
from axonhand.tools.annotate import make_server


def test_annotation_round_trips_through_a_file(tmp_path: Path) -> None:
    video = tmp_path / "clip.mp4"
    note = TapAnnotation("clip.mp4", (0.5, 1.0, 1.6), annotator="JC", note="dim light")
    path = tapmarks.save(video, note)
    assert path.name == "clip.mp4.taps.json"
    assert tapmarks.load(video) == note
    assert tapmarks.load(tmp_path / "other.mp4") is None


@pytest.mark.parametrize("times", [(1.0, 1.0), (2.0, 1.0), (-0.1, 1.0)])
def test_annotation_rejects_times_out_of_order(times: tuple[float, ...]) -> None:
    with pytest.raises(AnnotationError):
        TapAnnotation("clip.mp4", times)


def test_an_old_widest_opening_file_is_refused_with_an_explanation() -> None:
    old = '{"version": 1, "video": "a.mp4", "tap_times": [1.0, 2.0]}'
    with pytest.raises(AnnotationError, match="marking again"):
        TapAnnotation.from_json(old)


@pytest.mark.parametrize("text", ["", "{}", "[1]", '{"version": 9, "video": "a", "tap_times": []}'])
def test_annotation_rejects_bad_files(text: str) -> None:
    with pytest.raises(AnnotationError):
        TapAnnotation.from_json(text)


def test_matching_is_one_to_one_and_reports_the_signed_error() -> None:
    marked = np.array([1.0, 2.0, 3.0])
    found = np.array([1.02, 2.5, 2.98, 9.0])
    m = match_taps(found, marked, tolerance=0.1)
    assert m.found_index == (0, 2) and m.marked_index == (0, 2)
    assert m.errors == pytest.approx([0.02, -0.02])
    assert (m.extra, m.missed) == (2, 1)


def test_one_found_tap_cannot_claim_two_marked_taps() -> None:
    m = match_taps(np.array([1.0]), np.array([0.98, 1.03]), tolerance=0.1)
    assert len(m.found_index) == 1 and m.missed == 1 and m.extra == 0
    assert m.marked_index == (0,)


@given(st.lists(st.floats(0, 100), min_size=0, max_size=12, unique=True))
def test_identical_times_all_match_with_zero_error(times: list[float]) -> None:
    arr = np.sort(np.array(times, dtype=np.float64))
    m = match_taps(arr, arr, tolerance=0.0)
    assert (m.extra, m.missed) == (0, 0) and np.all(m.errors == 0)


def test_tolerance_has_a_floor_of_two_frames() -> None:
    assert tolerance_seconds(np.array([0.0, 0.1, 0.2]), 30.0) == pytest.approx(2 / 30)
    assert tolerance_seconds(np.arange(5, dtype=np.float64), 30.0) == pytest.approx(0.5)


def test_gap_counts_share_the_definition_used_by_the_analysis() -> None:
    times = np.array([0.0, 0.5, 1.0, 1.5, 2.3, 2.8, 5.0, 5.5])
    params = TappingParameters()
    assert gap_counts(times, params) == (1, 1)
    assert gap_counts(times[:1], params) == (0, 0)


def test_perfect_marks_give_no_error() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.5, taps=20, fps=60.0))
    marked = tuple(float(t) for t in analyse(data.track).contact_times)
    score = score_clip("c.mp4", data.track, marked)
    assert isinstance(score, ClipScore)
    assert (score.missed, score.extra, score.count_error) == (0, 0, 0)
    assert score.rate_error_percent == pytest.approx(0.0)


def test_a_person_marking_late_shows_as_a_bias() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.0, taps=20, fps=60.0))
    marked = tuple(float(t) + 0.05 for t in analyse(data.track).contact_times)
    score = score_clip("c.mp4", data.track, marked)
    assert isinstance(score, ClipScore)
    assert score.missed == 0
    assert np.mean(score.timing_errors) == pytest.approx(-0.05, abs=0.001)


def test_a_clip_the_software_cannot_analyse_is_a_failure() -> None:
    track = LandmarkTrack(30.0, np.full((90, 21, 3), np.nan))
    assert isinstance(score_clip("c.mp4", track, (0.5, 1.0, 1.5, 2.0)), Failure)


def test_too_few_marks_are_refused() -> None:
    data = generate_tapping(TapSpec(taps=10))
    with pytest.raises(ValueError):
        score_clip("c.mp4", data.track, (1.0, 2.0))


def test_report_counts_failures_against_the_software() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.0, taps=20, fps=60.0))
    marked = tuple(float(t) for t in analyse(data.track).contact_times)
    score = score_clip("good.mp4", data.track, marked)
    assert isinstance(score, ClipScore)
    summary = summarise([score], [Failure("bad.mp4", "no hand")])
    assert summary.clips == 2 and summary.failures == 1
    assert summary.recall == pytest.approx(1.0)
    assert ("at least 10 clips, none failed", False) in verdicts(summary)
    text = markdown([score], [Failure("bad.mp4", "no hand")], "test pipeline")
    assert "bad.mp4: no hand" in text and "NOT MET" in text
    assert json.loads(to_json([score], [], "p"))["summary"]["clips"] == 1


def _get(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=10) as response:
        return bytes(response.read())


def test_annotation_page_serves_and_saves_beside_the_video(tmp_path: Path) -> None:
    video = tmp_path / "clip.mp4"
    render_video(generate_tapping(TapSpec(taps=6, fps=30.0)).track, video)
    server = make_server(video)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        base = f"http://127.0.0.1:{server.server_address[1]}"
        assert b"Mark the taps" in _get(base + "/")
        info = json.loads(_get(base + "/info"))
        assert info["video"] == "clip.mp4" and info["fps"] == pytest.approx(30.0)
        assert _get(base + "/video") == video.read_bytes()
        ranged = urllib.request.Request(base + "/video", headers={"Range": "bytes=0-9"})
        with urllib.request.urlopen(ranged, timeout=10) as response:
            assert response.status == 206 and len(response.read()) == 10
        body = json.dumps({"tap_times": [0.5, 1.0, 1.5], "annotator": "JC"}).encode()
        post = urllib.request.Request(base + "/save", data=body, method="POST")
        post.add_header("Content-Length", str(len(body)))
        assert json.loads(urllib.request.urlopen(post, timeout=10).read())["path"]
        saved = tapmarks.load(video)
        assert saved is not None and saved.times == (0.5, 1.0, 1.5)
        bad = urllib.request.Request(base + "/save", data=b'{"tap_times": [2, 1]}', method="POST")
        with pytest.raises(urllib.error.HTTPError) as caught:
            urllib.request.urlopen(bad, timeout=10)
        assert caught.value.code == 400
    finally:
        server.shutdown()
        server.server_close()


def test_unmatched_taps_are_listed_by_time() -> None:
    data = generate_tapping(TapSpec(rate_hz=2.0, taps=20, fps=60.0))
    found = [float(t) for t in analyse(data.track).contact_times]
    marked = tuple(sorted([*found[:-1], found[-1] + 1.0]))
    score = score_clip("c.mp4", data.track, marked)
    assert isinstance(score, ClipScore)
    assert score.extra_times == pytest.approx((found[-1],))
    assert score.missed_times == pytest.approx((found[-1] + 1.0,))
    assert "marked but not found at" in markdown([score], [], "p")


def test_two_markers_who_agree_and_one_who_skips_a_tap() -> None:
    from axonhand.evaluation.agreement import compare_marks, markdown, summarise

    first = tuple(0.5 * k for k in range(1, 11))
    second = tuple(t + 0.03 for t in first[:-3] + first[-2:])
    item = compare_marks("c.mp4", first, second, fps=30.0)
    assert (item.first_count, item.second_count, item.hits) == (10, 9, 9)
    assert item.only_first == pytest.approx((first[-3],)) and item.only_second == ()
    s = summarise([item])
    assert s.agreement == pytest.approx(9 / 10)
    assert s.bias_ms == pytest.approx(30.0, abs=0.1) and s.p95_abs_ms == pytest.approx(
        30.0, abs=0.1
    )
    assert "Agreement between two markers" in markdown([item])


def test_marker_comparison_needs_four_marks_from_the_first_person() -> None:
    from axonhand.evaluation.agreement import compare_marks

    with pytest.raises(ValueError):
        compare_marks("c.mp4", (1.0, 2.0), (1.0, 2.0), fps=30.0)


def test_compare_markers_command_reads_two_folders(tmp_path: Path) -> None:
    from axonhand.compare_markers import main

    for name, shift in (("a", 0.0), ("b", 0.04)):
        folder = tmp_path / name
        folder.mkdir()
        render_video(generate_tapping(TapSpec(taps=8, fps=30.0)).track, folder / "c.mp4")
        times = tuple(0.5 * k + shift for k in range(1, 9))
        tapmarks.save(folder / "c.mp4", TapAnnotation("c.mp4", times, annotator=name))
    out = tmp_path / "out"
    assert main([str(tmp_path / "a"), str(tmp_path / "b"), "--out", str(out)]) == 0
    assert "Agreement between two markers" in (out / "agreement.md").read_text()
    empty = tmp_path / "empty"
    empty.mkdir()
    assert main([str(empty), str(empty), "--out", str(out)]) == 1
