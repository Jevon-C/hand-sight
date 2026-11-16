"""Tracker, video reading and the model wrapper, with stand-in models."""

from __future__ import annotations

import os
from pathlib import Path

import numpy as np
import pytest
from numpy.typing import NDArray

from axonhand.hand import rois
from axonhand.hand.runtime import ModelError, open_model
from axonhand.hand.tracker import HandTracker
from axonhand.hand.video import TrackingError, read_frames, track_video, upright
from axonhand.synthetic import TapSpec, generate_tapping, render_video

Frame = NDArray[np.uint8]


class StubPalm:
    """A palm detector that sees one palm at the middle of the frame, or nothing."""

    def __init__(self, sees_palm: bool = True) -> None:
        self.sees_palm = sees_palm
        self.calls = 0

    def run(self, blob: NDArray[np.float32]) -> list[NDArray[np.float32]]:
        assert blob.shape == (1, 192, 192, 3)
        self.calls += 1
        raw = np.zeros((1, 2016, 18), dtype=np.float32)
        logits = np.full((1, 2016, 1), -20.0, dtype=np.float32)
        if self.sees_palm:
            index = 500
            anchor = rois.palm_anchors()[index] * rois.PALM_INPUT
            raw[0, index, 0:2] = (96.0 - anchor[0], 96.0 - anchor[1])
            raw[0, index, 2:4] = (40.0, 40.0)
            for k in range(7):
                raw[0, index, 4 + 2 * k] = 96.0 - anchor[0]
                raw[0, index, 5 + 2 * k] = 96.0 - anchor[1] + (3 - k) * 3.0
            logits[0, index, 0] = 6.0
        return [raw, logits]


TEMPLATE = np.random.default_rng(1).uniform(60, 160, size=(21, 3))


class StubLandmarks:
    """A landmark model that always reports the same hand shape, with scripted scores."""

    def __init__(self, scores: list[float]) -> None:
        self.scores = list(scores)
        self.calls = 0

    def run(self, blob: NDArray[np.float32]) -> list[NDArray[np.float32]]:
        assert blob.shape == (1, 224, 224, 3)
        score = self.scores[min(self.calls, len(self.scores) - 1)]
        self.calls += 1
        return [
            TEMPLATE.reshape(1, 63).astype(np.float32),
            np.array([[score]], dtype=np.float32),
            np.array([[0.5]], dtype=np.float32),
            np.zeros((1, 63), dtype=np.float32),
        ]


def blank(width: int = 320, height: int = 240) -> Frame:
    return np.full((height, width, 3), 120, dtype=np.uint8)


def expected_first_rect(width: int = 320, height: int = 240) -> rois.Rect:
    mapping, _, _ = rois.letterbox(width, height)
    palm = rois.decode_palms(*_palm_arrays(), mapping)[0]
    return rois.rect_from_palm(palm)


def _palm_arrays() -> tuple[NDArray[np.float32], NDArray[np.float32]]:
    raw, logits = StubPalm().run(np.zeros((1, 192, 192, 3), np.float32))
    return raw.reshape(-1, 18), logits.reshape(-1)


def test_the_first_frame_detects_the_palm_then_maps_the_landmarks_back() -> None:
    palm, landmarks = StubPalm(), StubLandmarks([0.99])
    tracker = HandTracker(palm, landmarks)
    result = tracker.process(blank())
    assert result is not None
    assert result.redetected and result.confidence == pytest.approx(0.99)
    expected = rois.crop_to_image(expected_first_rect(), TEMPLATE)
    assert np.allclose(result.points, expected)
    assert palm.calls == 1 and landmarks.calls == 1


def test_later_frames_follow_the_hand_without_running_the_detector() -> None:
    palm, landmarks = StubPalm(), StubLandmarks([0.99])
    tracker = HandTracker(palm, landmarks)
    first = tracker.process(blank())
    second = tracker.process(blank())
    assert first is not None and second is not None
    assert palm.calls == 1 and landmarks.calls == 2
    assert not second.redetected
    # The second crop is placed from the first frame's landmarks, so it is not the first one.
    follow_up = rois.rect_from_landmarks(first.points)
    assert np.allclose(second.points, rois.crop_to_image(follow_up, TEMPLATE))


def test_a_low_score_while_following_makes_the_tracker_look_again() -> None:
    palm, landmarks = StubPalm(), StubLandmarks([0.99, 0.1, 0.99])
    tracker = HandTracker(palm, landmarks)
    assert tracker.process(blank()) is not None
    result = tracker.process(blank())
    assert result is not None and result.redetected
    assert palm.calls == 2


def test_no_palm_means_no_hand() -> None:
    tracker = HandTracker(StubPalm(sees_palm=False), StubLandmarks([0.99]))
    assert tracker.process(blank()) is None


def test_a_hand_that_cannot_be_confirmed_is_dropped_and_looked_for_again() -> None:
    palm, landmarks = StubPalm(), StubLandmarks([0.1])
    tracker = HandTracker(palm, landmarks)
    assert tracker.process(blank()) is None
    assert tracker.process(blank()) is None
    assert palm.calls == 2


def test_reset_forgets_the_hand() -> None:
    palm = StubPalm()
    tracker = HandTracker(palm, StubLandmarks([0.99]))
    tracker.process(blank())
    tracker.reset()
    tracker.process(blank())
    assert palm.calls == 2


@pytest.mark.parametrize(
    ("degrees", "expected_shape"),
    [(0, (2, 3)), (90, (3, 2)), (180, (2, 3)), (270, (3, 2)), (-90, (3, 2)), (360, (2, 3))],
)
def test_upright_applies_the_display_rotation(
    degrees: float, expected_shape: tuple[int, int]
) -> None:
    image = np.arange(18, dtype=np.uint8).reshape(2, 3, 3)
    assert upright(image, degrees).shape[:2] == expected_shape


def test_a_quarter_turn_anticlockwise_puts_the_right_edge_on_top() -> None:
    image = np.zeros((2, 3, 3), dtype=np.uint8)
    image[0, 2] = 255  # top right
    turned = upright(image, 90)
    assert turned[0, 0, 0] == 255  # now top left


def _video(tmp_path: Path, taps: int = 4) -> Path:
    path = tmp_path / "clip.mp4"
    render_video(generate_tapping(TapSpec(taps=taps, rate_hz=2.0)).track, path)
    return path


def test_frames_and_rate_come_from_the_file(tmp_path: Path) -> None:
    fps, frames = read_frames(_video(tmp_path))
    listed = list(frames)
    assert fps == pytest.approx(60.0)
    assert len(listed) > 60
    assert listed[0].shape == (240, 320, 3) and listed[0].dtype == np.uint8


def test_a_file_that_is_not_a_video_is_a_tracking_error(tmp_path: Path) -> None:
    junk = tmp_path / "junk.mp4"
    junk.write_bytes(b"not a video at all")
    with pytest.raises(TrackingError):
        read_frames(junk)
    with pytest.raises(TrackingError):
        read_frames(tmp_path / "missing.mp4")


def test_a_whole_video_gives_one_row_per_frame_and_reports_progress(tmp_path: Path) -> None:
    path = _video(tmp_path)
    seen: list[int] = []
    track, tracker = track_video(path, StubPalm(), StubLandmarks([0.99]), seen.append)
    fps, frames = read_frames(path)
    count = sum(1 for _ in frames)
    assert track.frames == count and track.fps == pytest.approx(fps)
    assert seen == list(range(1, count + 1))
    assert np.isfinite(track.points).all()
    assert tracker.detections == 1
    assert set(tracker.timing) == {"decode", "detect", "landmark"}
    assert all(v >= 0 for v in tracker.timing.values()) and tracker.timing["decode"] > 0


def test_a_video_without_a_hand_is_a_tracking_error_a_clinician_can_read(tmp_path: Path) -> None:
    with pytest.raises(TrackingError, match="No hand was found"):
        track_video(_video(tmp_path), StubPalm(sees_palm=False), StubLandmarks([0.99]))


def test_frames_with_no_hand_are_nan_rows(tmp_path: Path) -> None:
    class Flicker(StubLandmarks):
        def run(self, blob: NDArray[np.float32]) -> list[NDArray[np.float32]]:
            self.scores = [0.99 if self.calls % 5 else 0.0]
            return super().run(blob)

    track, _ = track_video(_video(tmp_path), StubPalm(), Flicker([0.99]))
    missing = np.isnan(track.points).all(axis=(1, 2))
    assert missing.any() and not missing.all()


def test_a_missing_model_file_is_a_model_error(tmp_path: Path) -> None:
    with pytest.raises(ModelError, match="not found"):
        open_model(tmp_path / "nope.onnx")


def test_a_file_that_is_not_a_model_is_a_model_error(tmp_path: Path) -> None:
    bad = tmp_path / "bad.onnx"
    bad.write_bytes(b"not a model")
    with pytest.raises(ModelError, match="cannot load"):
        open_model(bad, "cpu")


def _models() -> Path | None:
    for candidate in (os.environ.get("AXONHAND_MODELS"), "models", "../models"):
        if candidate and (Path(candidate) / "palm_detection_mediapipe_2023feb.onnx").is_file():
            return Path(candidate)
    return None


needs_models = pytest.mark.skipif(_models() is None, reason="hand model files not found")


@needs_models
def test_the_real_models_load_and_run_on_the_cpu() -> None:
    folder = _models()
    assert folder is not None
    palm = open_model(folder / "palm_detection_mediapipe_2023feb.onnx", "cpu")
    landmarks = open_model(folder / "handpose_estimation_mediapipe_2023feb.onnx", "cpu")
    assert palm.device == "CPU" and landmarks.device == "CPU" and palm.note == ""
    raw, scores = palm.run(np.zeros((1, 192, 192, 3), dtype=np.float32))
    assert raw.shape == (1, 2016, 18) and scores.shape == (1, 2016, 1)
    points, score, handed, world = landmarks.run(np.zeros((1, 224, 224, 3), dtype=np.float32))
    assert points.shape == (1, 63) and world.shape == (1, 63)
    assert score.shape == (1, 1) and handed.shape == (1, 1)


@needs_models
def test_a_picture_with_no_hand_in_it_gives_no_hand() -> None:
    folder = _models()
    assert folder is not None
    palm = open_model(folder / "palm_detection_mediapipe_2023feb.onnx", "cpu")
    landmarks = open_model(folder / "handpose_estimation_mediapipe_2023feb.onnx", "cpu")
    rng = np.random.default_rng(0)
    plain = np.full((480, 640, 3), 90, dtype=np.uint8)
    noise = rng.integers(0, 255, size=(480, 640, 3), dtype=np.uint8)
    for picture in (plain, noise):
        assert HandTracker(palm, landmarks).process(picture) is None


@needs_models
def test_the_command_line_reports_an_unusable_video_with_status_2(tmp_path: Path) -> None:
    from axonhand import hand_model

    folder = _models()
    assert folder is not None
    code = hand_model.main(
        [
            str(_video(tmp_path)),
            str(tmp_path / "out.npz"),
            "--models",
            str(folder),
            "--device",
            "cpu",
        ]
    )
    assert code == 2  # coloured dots on grey are not a hand


@needs_models
def test_the_message_for_the_person_is_marked_on_stderr(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    from axonhand import hand_model

    folder = _models()
    assert folder is not None
    hand_model.main(
        [str(_video(tmp_path)), str(tmp_path / "o.npz"), "--models", str(folder), "--device", "cpu"]
    )
    lines = [
        x for x in capsys.readouterr().err.splitlines() if x.startswith(hand_model.MESSAGE_PREFIX)
    ]
    assert len(lines) == 1 and "No hand was found" in lines[0]


def test_the_command_line_reports_missing_models_with_status_1(tmp_path: Path) -> None:
    from axonhand import hand_model

    code = hand_model.main(
        [str(_video(tmp_path)), str(tmp_path / "out.npz"), "--models", str(tmp_path / "none")]
    )
    assert code == 1


@needs_models
def test_asking_for_the_npu_either_gets_it_or_says_why_not() -> None:
    folder = _models()
    assert folder is not None
    model = open_model(folder / "palm_detection_mediapipe_2023feb.onnx", "auto")
    assert (model.device == "NPU" and model.note == "") or (model.device == "CPU" and model.note)
