"""Find the hand in one frame at a time: detect once, then follow it from frame to frame."""

from __future__ import annotations

import time
from dataclasses import dataclass

import numpy as np
from numpy.typing import NDArray
from PIL import Image

from axonhand.hand import rois
from axonhand.hand.runtime import Model

_HAND_PRESENT = 0.5
UInt8Image = NDArray[np.uint8]


@dataclass(frozen=True)
class HandFrame:
    """What was found in one frame."""

    points: NDArray[np.float64]
    """Shape (21, 3): x and y in image pixels, depth in pixels relative to the wrist."""
    confidence: float
    """The landmark model's score that a hand is present, 0 to 1."""
    redetected: bool
    """True when the palm detector had to find the hand again in this frame."""


def _blob(image: Image.Image) -> NDArray[np.float32]:
    array = np.asarray(image, dtype=np.float32) / 255.0
    return array[np.newaxis]


class HandTracker:
    """Follows one hand through a video. Create one per video; it keeps the last position."""

    def __init__(self, palm_model: Model, landmark_model: Model) -> None:
        """Use the given open models."""
        self._palm = palm_model
        self._landmarks = landmark_model
        self._rect: rois.Rect | None = None
        self.detections = 0
        """How many times the palm detector was run, for reporting."""
        self.timing = {"decode": 0.0, "detect": 0.0, "landmark": 0.0}
        """Seconds spent decoding frames, in the palm detector and in the landmark stage."""

    def reset(self) -> None:
        """Forget the hand, for example after a cut in the video."""
        self._rect = None

    def _detect(self, image: Image.Image) -> rois.Rect | None:
        mapping, new_w, new_h = rois.letterbox(*image.size)
        canvas = Image.new("RGB", (rois.PALM_INPUT, rois.PALM_INPUT), (0, 0, 0))
        resized = image.resize((new_w, new_h), Image.Resampling.BILINEAR)
        canvas.paste(resized, (round(mapping.pad_x), round(mapping.pad_y)))
        started = time.perf_counter()
        raw, logits = self._palm.run(_blob(canvas))[:2]
        self.timing["detect"] += time.perf_counter() - started
        self.detections += 1
        palms = rois.decode_palms(raw.reshape(-1, 18), logits.reshape(-1), mapping)
        return rois.rect_from_palm(palms[0]) if palms else None

    def _landmark(
        self, image: Image.Image, rect: rois.Rect
    ) -> tuple[NDArray[np.float64], float] | None:
        started = time.perf_counter()
        crop = image.transform(
            (rois.LANDMARK_INPUT, rois.LANDMARK_INPUT),
            Image.Transform.AFFINE,
            rois.crop_matrix(rect),
            resample=Image.Resampling.BILINEAR,
        )
        outputs = self._landmarks.run(_blob(crop))
        self.timing["landmark"] += time.perf_counter() - started
        score = float(outputs[1].reshape(-1)[0])
        if score < _HAND_PRESENT:
            return None
        local = outputs[0].reshape(21, 3).astype(np.float64)
        return rois.crop_to_image(rect, local), score

    def process(self, frame: UInt8Image) -> HandFrame | None:
        """Find the hand in an RGB frame of shape (height, width, 3), or return ``None``."""
        image = Image.fromarray(frame)
        redetected = False
        found = None
        if self._rect is not None:
            found = self._landmark(image, self._rect)
        if found is None:
            # Lost, or never found: look for the palm again, once, in this same frame.
            self._rect = self._detect(image)
            redetected = True
            if self._rect is not None:
                found = self._landmark(image, self._rect)
        if found is None:
            self._rect = None
            return None
        points, score = found
        self._rect = rois.rect_from_landmarks(points)
        return HandFrame(points, score, redetected)
