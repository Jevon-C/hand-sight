"""Geometry of the two-stage hand pipeline (the MediaPipe design), with no model in it.

Everything here is in *continuous image coordinates*: the top-left corner of the top-left pixel
is (0, 0), so a pixel's centre is at (i + 0.5, j + 0.5). x runs right, y runs down. Angles are in
radians; a rectangle's rotation is the angle that turns an upright hand (wrist at the bottom,
fingers up) into the hand in the image, positive clockwise on screen.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray

PALM_INPUT = 192
LANDMARK_INPUT = 224
PALM_KEYPOINTS = 7

# Palm-detector keypoints used for the rotation: wrist centre and middle-finger base.
_PALM_WRIST, _PALM_MIDDLE = 0, 2
# Landmark numbering (MediaPipe): wrist and middle-finger base.
_WRIST, _MIDDLE_MCP = 0, 9

_PALM_SCALE, _PALM_SHIFT_Y = 2.6, -0.5
_TRACK_SCALE, _TRACK_SHIFT_Y = 2.0, -0.1
_MAX_LOGIT = 100.0

FloatArray = NDArray[np.float64]


@dataclass(frozen=True)
class Rect:
    """A square, possibly rotated, region of the image."""

    cx: float
    cy: float
    size: float
    rotation: float


@dataclass(frozen=True)
class Palm:
    """One detected palm, in image coordinates."""

    score: float
    cx: float
    cy: float
    width: float
    height: float
    keypoints: FloatArray
    """Shape (7, 2): wrist centre, index, middle, ring and little base, thumb base and joint."""


@dataclass(frozen=True)
class Letterbox:
    """How an image was fitted into the square detector input."""

    scale: float
    pad_x: float
    pad_y: float

    def to_image(self, x: FloatArray, y: FloatArray) -> tuple[FloatArray, FloatArray]:
        """Map detector-input pixel positions (0..192) back to image positions."""
        return (x - self.pad_x) / self.scale, (y - self.pad_y) / self.scale


def letterbox(width: int, height: int, size: int = PALM_INPUT) -> tuple[Letterbox, int, int]:
    """Fit a ``width`` x ``height`` image inside ``size`` x ``size`` keeping its shape.

    Returns the mapping plus the resized width and height. The image is centred (to a whole pixel);
    the rest of the square is padding.
    """
    if width <= 0 or height <= 0:
        raise ValueError("image size must be positive")
    scale = size / max(width, height)
    new_w = max(1, round(width * scale))
    new_h = max(1, round(height * scale))
    return Letterbox(scale, float((size - new_w) // 2), float((size - new_h) // 2)), new_w, new_h


def palm_anchors() -> FloatArray:
    """The 2016 anchor centres of the palm detector, as fractions of its input (0..1).

    Four layers at strides 8, 16, 16 and 16. Layers with the same stride share a grid, and each
    layer puts two anchors in every cell: 24x24x2 + 12x12x6 = 2016.
    """
    strides = (8, 16, 16, 16)
    anchors: list[tuple[float, float]] = []
    layer = 0
    while layer < len(strides):
        last = layer
        while last < len(strides) and strides[last] == strides[layer]:
            last += 1
        cells = math.ceil(PALM_INPUT / strides[layer])
        per_cell = 2 * (last - layer)
        for y in range(cells):
            for x in range(cells):
                anchors.extend([((x + 0.5) / cells, (y + 0.5) / cells)] * per_cell)
        layer = last
    return np.asarray(anchors, dtype=np.float64)


def _iou(box: FloatArray, others: FloatArray) -> FloatArray:
    x1 = np.maximum(box[0], others[:, 0])
    y1 = np.maximum(box[1], others[:, 1])
    x2 = np.minimum(box[2], others[:, 2])
    y2 = np.minimum(box[3], others[:, 3])
    inter = np.clip(x2 - x1, 0, None) * np.clip(y2 - y1, 0, None)
    area = (box[2] - box[0]) * (box[3] - box[1])
    areas = (others[:, 2] - others[:, 0]) * (others[:, 3] - others[:, 1])
    union = area + areas - inter
    return np.where(union > 0, inter / np.where(union > 0, union, 1.0), 0.0)


def decode_palms(
    raw: NDArray[np.floating],
    logits: NDArray[np.floating],
    mapping: Letterbox,
    *,
    score_threshold: float = 0.5,
    iou_threshold: float = 0.3,
) -> list[Palm]:
    """Turn the detector's raw output into palms, best first, with overlaps removed.

    ``raw`` has shape (2016, 18): box centre offset, box size and seven keypoint offsets, in
    detector-input pixels relative to each anchor. ``logits`` has shape (2016,).
    """
    anchors = palm_anchors()
    if raw.shape != (anchors.shape[0], 18) or logits.shape != (anchors.shape[0],):
        raise ValueError("unexpected detector output shape")
    scores = 1.0 / (1.0 + np.exp(-np.clip(logits.astype(np.float64), -_MAX_LOGIT, _MAX_LOGIT)))
    keep = np.flatnonzero(scores >= score_threshold)
    if keep.size == 0:
        return []
    raw64 = raw[keep].astype(np.float64)
    ax = anchors[keep, 0] * PALM_INPUT
    ay = anchors[keep, 1] * PALM_INPUT
    cx, cy = raw64[:, 0] + ax, raw64[:, 1] + ay
    w, h = raw64[:, 2], raw64[:, 3]
    boxes = np.stack([cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2], axis=1)
    order = np.argsort(-scores[keep])
    chosen: list[int] = []
    alive = np.ones(order.size, dtype=bool)
    for rank in range(order.size):
        if not alive[rank]:
            continue
        chosen.append(int(order[rank]))
        overlap = _iou(boxes[order[rank]], boxes[order])
        alive &= overlap <= iou_threshold
        alive[rank] = False
    palms: list[Palm] = []
    for i in chosen:
        kx = raw64[i, 4::2] + ax[i]
        ky = raw64[i, 5::2] + ay[i]
        ix, iy = mapping.to_image(kx, ky)
        bx, by = mapping.to_image(np.array([cx[i]]), np.array([cy[i]]))
        palms.append(
            Palm(
                score=float(scores[keep][i]),
                cx=float(bx[0]),
                cy=float(by[0]),
                width=float(w[i] / mapping.scale),
                height=float(h[i] / mapping.scale),
                keypoints=np.stack([ix, iy], axis=1),
            )
        )
    return palms


def _hand_rotation(wrist: FloatArray, middle: FloatArray) -> float:
    """Rotation that makes the wrist-to-middle-finger direction point up, in (-pi, pi]."""
    rotation = math.pi / 2 - math.atan2(-(middle[1] - wrist[1]), middle[0] - wrist[0])
    return rotation - 2 * math.pi * math.floor((rotation + math.pi) / (2 * math.pi))


def _shifted(
    cx: float, cy: float, height: float, rotation: float, shift_y: float
) -> tuple[float, float]:
    """Move a centre along the hand's own up direction by ``shift_y`` times the height."""
    return (
        cx - height * shift_y * math.sin(rotation),
        cy + height * shift_y * math.cos(rotation),
    )


def rect_from_palm(palm: Palm) -> Rect:
    """The square region to give the landmark model, from a detected palm."""
    rotation = _hand_rotation(palm.keypoints[_PALM_WRIST], palm.keypoints[_PALM_MIDDLE])
    cx, cy = _shifted(palm.cx, palm.cy, palm.height, rotation, _PALM_SHIFT_Y)
    return Rect(cx, cy, max(palm.width, palm.height) * _PALM_SCALE, rotation)


def rect_from_landmarks(points: FloatArray) -> Rect:
    """The region for the next frame, from this frame's landmarks (shape (21, 2) or (21, 3)).

    The box is measured in the hand's own turned frame, so it does not grow when the hand tilts.
    """
    xy = points[:, :2]
    rotation = _hand_rotation(xy[_WRIST], xy[_MIDDLE_MCP])
    cos, sin = math.cos(rotation), math.sin(rotation)
    # Turn the image coordinates into the hand's frame (undo the rotation).
    local_x = xy[:, 0] * cos + xy[:, 1] * sin
    local_y = -xy[:, 0] * sin + xy[:, 1] * cos
    lo_x, hi_x = float(local_x.min()), float(local_x.max())
    lo_y, hi_y = float(local_y.min()), float(local_y.max())
    mid_x, mid_y = (lo_x + hi_x) / 2, (lo_y + hi_y) / 2
    width, height = hi_x - lo_x, hi_y - lo_y
    cx = mid_x * cos - mid_y * sin
    cy = mid_x * sin + mid_y * cos
    cx, cy = _shifted(cx, cy, height, rotation, _TRACK_SHIFT_Y)
    return Rect(cx, cy, max(width, height) * _TRACK_SCALE, rotation)


def crop_matrix(
    rect: Rect, size: int = LANDMARK_INPUT
) -> tuple[float, float, float, float, float, float]:
    """Affine coefficients (a, b, c, d, e, f) taking a crop position (u, v) to an image position.

    ``x = a*u + b*v + c`` and ``y = d*u + e*v + f``, with (u, v) continuous in 0..size. This is
    the form Pillow's affine transform wants, and the one :func:`crop_to_image` uses.
    """
    scale = rect.size / size
    cos, sin = math.cos(rect.rotation), math.sin(rect.rotation)
    half = size / 2.0
    a, b = scale * cos, -scale * sin
    d, e = scale * sin, scale * cos
    c = rect.cx - (a * half + b * half)
    f = rect.cy - (d * half + e * half)
    return a, b, c, d, e, f


def crop_to_image(rect: Rect, points: FloatArray, size: int = LANDMARK_INPUT) -> FloatArray:
    """Map landmark positions in crop pixels back to the image.

    ``points`` has shape (n, 3): x and y in crop pixels and a depth in the same units. x and y are
    placed in the image; depth is only rescaled, because it is relative to the wrist.
    """
    a, b, c, d, e, f = crop_matrix(rect, size)
    out = np.empty_like(points, dtype=np.float64)
    out[:, 0] = a * points[:, 0] + b * points[:, 1] + c
    out[:, 1] = d * points[:, 0] + e * points[:, 1] + f
    out[:, 2] = points[:, 2] * (rect.size / size)
    return out
