"""The hand pipeline's geometry, tested exactly and without any model."""

from __future__ import annotations

import math

import numpy as np
import pytest
from hypothesis import given, settings
from hypothesis import strategies as st
from numpy.typing import NDArray
from PIL import Image

from axonhand.hand import rois


def test_there_are_2016_anchors_inside_the_unit_square() -> None:
    anchors = rois.palm_anchors()
    assert anchors.shape == (2016, 2)
    assert anchors.min() > 0 and anchors.max() < 1


def test_anchor_layout_matches_the_detector() -> None:
    anchors = rois.palm_anchors()
    # 24 x 24 cells with 2 anchors each come first; the first cell centre is half a cell in.
    assert np.allclose(anchors[0], (0.5 / 24, 0.5 / 24))
    assert np.allclose(anchors[1], anchors[0])
    assert np.allclose(anchors[2], (1.5 / 24, 0.5 / 24))
    # then 12 x 12 cells with 6 anchors each.
    assert np.allclose(anchors[1152], (0.5 / 12, 0.5 / 12))
    assert np.allclose(anchors[1152 + 5], anchors[1152])
    assert np.allclose(anchors[1152 + 6], (1.5 / 12, 0.5 / 12))


def test_letterbox_keeps_the_shape_and_maps_back() -> None:
    mapping, new_w, new_h = rois.letterbox(1920, 1080)
    assert (new_w, new_h) == (192, 108)
    assert mapping.pad_x == 0 and mapping.pad_y == 42
    x, y = mapping.to_image(np.array([96.0]), np.array([96.0]))
    assert (x[0], y[0]) == pytest.approx((960.0, 540.0))


def test_letterbox_rejects_an_empty_image() -> None:
    with pytest.raises(ValueError):
        rois.letterbox(0, 10)


def _raw_with(index: int, centre: tuple[float, float], size: tuple[float, float]) -> np.ndarray:
    raw = np.zeros((2016, 18), dtype=np.float32)
    anchor = rois.palm_anchors()[index] * rois.PALM_INPUT
    raw[index, 0:2] = (centre[0] - anchor[0], centre[1] - anchor[1])
    raw[index, 2:4] = size
    for k in range(7):
        # keypoints on a vertical line through the centre: wrist lowest, middle base higher.
        raw[index, 4 + 2 * k] = centre[0] - anchor[0]
        raw[index, 5 + 2 * k] = centre[1] - anchor[1] + (k - 3) * 2
    return raw


def test_a_confident_anchor_becomes_a_palm_in_image_coordinates() -> None:
    mapping, _, _ = rois.letterbox(1920, 1080)
    raw = _raw_with(500, (96.0, 96.0), (40.0, 40.0))
    logits = np.full(2016, -20.0, dtype=np.float32)
    logits[500] = 5.0
    palms = rois.decode_palms(raw, logits, mapping)
    assert len(palms) == 1
    palm = palms[0]
    assert palm.score == pytest.approx(1 / (1 + math.exp(-5)))
    assert (palm.cx, palm.cy) == pytest.approx((960.0, 540.0))
    assert (palm.width, palm.height) == pytest.approx((400.0, 400.0))
    assert palm.keypoints.shape == (7, 2)


def test_overlapping_boxes_are_merged_and_distant_ones_kept() -> None:
    mapping, _, _ = rois.letterbox(192, 192)
    raw = _raw_with(500, (60.0, 60.0), (40.0, 40.0))
    near = _raw_with(501, (62.0, 60.0), (40.0, 40.0))
    far = _raw_with(900, (150.0, 150.0), (40.0, 40.0))
    raw[501] = near[501]
    raw[900] = far[900]
    logits = np.full(2016, -20.0, dtype=np.float32)
    logits[[500, 501, 900]] = [4.0, 3.0, 2.0]
    palms = rois.decode_palms(raw, logits, mapping)
    assert len(palms) == 2
    assert palms[0].score > palms[1].score
    assert palms[0].cx == pytest.approx(60.0)


def test_nothing_above_the_threshold_gives_no_palms() -> None:
    mapping, _, _ = rois.letterbox(192, 192)
    assert rois.decode_palms(np.zeros((2016, 18), np.float32), np.full(2016, -5.0), mapping) == []


def test_a_wrong_output_shape_is_refused() -> None:
    mapping, _, _ = rois.letterbox(192, 192)
    with pytest.raises(ValueError):
        rois.decode_palms(np.zeros((10, 18), np.float32), np.zeros(10), mapping)


def _palm(wrist: tuple[float, float], middle: tuple[float, float]) -> rois.Palm:
    kp = np.zeros((7, 2))
    kp[0], kp[2] = wrist, middle
    return rois.Palm(0.9, 100.0, 100.0, 40.0, 60.0, kp)


def test_an_upright_hand_has_no_rotation_and_the_box_moves_up_and_grows() -> None:
    rect = rois.rect_from_palm(_palm((100, 150), (100, 50)))
    assert rect.rotation == pytest.approx(0.0)
    assert rect.size == pytest.approx(60.0 * 2.6)
    assert (rect.cx, rect.cy) == pytest.approx((100.0, 100.0 - 0.5 * 60.0))


def test_a_hand_pointing_right_turns_a_quarter_clockwise() -> None:
    rect = rois.rect_from_palm(_palm((50, 100), (150, 100)))
    assert rect.rotation == pytest.approx(math.pi / 2)
    # up for this hand is to the right on screen, so the box moves right.
    assert rect.cx == pytest.approx(100.0 + 0.5 * 60.0)
    assert rect.cy == pytest.approx(100.0)


@given(st.floats(-3.0, 3.0))
def test_the_rotation_follows_any_hand_direction(angle: float) -> None:
    # The hand axis points along (sin a, -cos a): a = 0 is straight up on screen.
    wrist = (100.0, 100.0)
    middle = (100.0 + 50 * math.sin(angle), 100.0 - 50 * math.cos(angle))
    assert rois.rect_from_palm(_palm(wrist, middle)).rotation == pytest.approx(angle, abs=1e-9)


def _hand_points(
    angle: float, scale: float = 1.0, centre: tuple[float, float] = (300, 300)
) -> NDArray[np.float64]:
    """A rough hand: wrist at the origin, fingers up, turned by ``angle``."""
    rng = np.random.default_rng(3)
    local = rng.uniform(-0.5, 0.5, size=(21, 2))
    local[:, 1] = -np.abs(local[:, 1]) - 0.2  # all above the wrist
    local[0] = (0.0, 0.0)
    local[9] = (0.0, -1.0)
    cos, sin = math.cos(angle), math.sin(angle)
    rotated = np.stack(
        [local[:, 0] * cos - local[:, 1] * sin, local[:, 0] * sin + local[:, 1] * cos], axis=1
    )
    pts = rotated * 100 * scale + np.array(centre)
    return np.concatenate([pts, np.zeros((21, 1))], axis=1)


@given(st.floats(-3.0, 3.0))
def test_the_tracking_box_does_not_grow_when_the_hand_tilts(angle: float) -> None:
    upright = rois.rect_from_landmarks(_hand_points(0.0))
    tilted = rois.rect_from_landmarks(_hand_points(angle))
    assert tilted.size == pytest.approx(upright.size, rel=1e-6)
    assert tilted.rotation == pytest.approx(angle, abs=1e-6)


def test_the_tracking_box_scales_with_the_hand() -> None:
    small = rois.rect_from_landmarks(_hand_points(0.4, scale=1.0))
    large = rois.rect_from_landmarks(_hand_points(0.4, scale=2.0))
    assert large.size == pytest.approx(2 * small.size, rel=1e-6)


@settings(max_examples=50)
@given(
    st.floats(50, 400),
    st.floats(50, 400),
    st.floats(40, 300),
    st.floats(-3.0, 3.0),
    st.lists(
        st.tuples(st.floats(0, 224), st.floats(0, 224), st.floats(-50, 50)), min_size=1, max_size=21
    ),
)
def test_crop_to_image_inverts_the_crop_mapping(cx, cy, size, rotation, pts) -> None:  # type: ignore[no-untyped-def]
    rect = rois.Rect(cx, cy, size, rotation)
    crop = np.asarray(pts, dtype=np.float64)
    image = rois.crop_to_image(rect, crop)
    # Going the other way by hand: subtract the centre, undo the rotation, scale to crop pixels.
    scale = size / rois.LANDMARK_INPUT
    dx, dy = image[:, 0] - cx, image[:, 1] - cy
    u = (dx * math.cos(rotation) + dy * math.sin(rotation)) / scale + rois.LANDMARK_INPUT / 2
    v = (-dx * math.sin(rotation) + dy * math.cos(rotation)) / scale + rois.LANDMARK_INPUT / 2
    assert np.allclose(u, crop[:, 0], atol=1e-6)
    assert np.allclose(v, crop[:, 1], atol=1e-6)
    assert np.allclose(image[:, 2], crop[:, 2] * scale)


@pytest.mark.parametrize("rotation", [0.0, 0.6, -1.2, math.pi / 2, 2.9])
def test_the_crop_really_samples_the_rotated_region(rotation: float) -> None:
    # A bright dot at a known image position must land where the mapping says in the crop.
    image = np.zeros((400, 400, 3), dtype=np.uint8)
    rect = rois.Rect(210.0, 190.0, 160.0, rotation)
    u_true, v_true = 150.0, 80.0  # a position in the 224 x 224 crop
    x, y = rois.crop_to_image(rect, np.array([[u_true, v_true, 0.0]]))[0, :2]
    xi, yi = int(x), int(y)
    image[yi - 3 : yi + 4, xi - 3 : xi + 4] = 255
    crop = Image.fromarray(image).transform(
        (224, 224),
        Image.Transform.AFFINE,
        rois.crop_matrix(rect),
        resample=Image.Resampling.BILINEAR,
    )
    grey = np.asarray(crop, dtype=np.float64)[:, :, 0]
    vv, uu = np.mgrid[0:224, 0:224]
    mass = grey.sum()
    assert mass > 0
    u = float(((uu + 0.5) * grey).sum() / mass)
    v = float(((vv + 0.5) * grey).sum() / mass)
    # The dot's centre is at pixel (xi + 0.5, yi + 0.5) in continuous coordinates.
    a, b, c, d, e, f = rois.crop_matrix(rect)
    det = a * e - b * d
    ex, ey = xi + 0.5 - c, yi + 0.5 - f
    u_exp = (e * ex - b * ey) / det
    v_exp = (-d * ex + a * ey) / det
    assert (u, v) == pytest.approx((u_exp, v_exp), abs=0.3)
