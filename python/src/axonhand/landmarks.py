"""Hand landmark tracks: the common format between models, plug-ins and tests.

A track holds the position of the 21 hand landmarks (the MediaPipe numbering) in every frame.
Distances are in image units for a real model and in palm lengths for synthetic data; the
analysis normalises by palm length either way. A landmark that was not found in a frame is NaN.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from numpy.typing import NDArray

WRIST = 0
THUMB_TIP = 4
INDEX_TIP = 8
INDEX_MCP = 5
MIDDLE_MCP = 9
RING_MCP = 13
PINKY_MCP = 17
FINGERTIPS = (4, 8, 12, 16, 20)
"""Thumb, index, middle, ring and little fingertips."""
PALM_POINTS = (0, 5, 9, 13, 17)
"""The wrist and the four finger bases: the points of the palm, which move as one."""
LANDMARK_COUNT = 21
_FORMAT_VERSION = 1


@dataclass(frozen=True)
class LandmarkTrack:
    """Positions of the hand landmarks over time.

    ``points`` has shape ``(frames, 21, 3)``; unused coordinates and missing landmarks are NaN.
    """

    fps: float
    points: NDArray[np.float64]

    def __post_init__(self) -> None:
        if not np.isfinite(self.fps) or self.fps <= 0:
            raise ValueError("fps must be positive and finite")
        if self.points.ndim != 3 or self.points.shape[1:] != (LANDMARK_COUNT, 3):
            raise ValueError(f"points must have shape (frames, {LANDMARK_COUNT}, 3)")

    @property
    def frames(self) -> int:
        """Number of frames."""
        return int(self.points.shape[0])

    @property
    def times(self) -> NDArray[np.float64]:
        """Time of each frame in seconds."""
        return np.arange(self.frames, dtype=np.float64) / self.fps

    def save(self, path: Path) -> None:
        """Write the track to a ``.npz`` file."""
        np.savez(path, version=_FORMAT_VERSION, fps=self.fps, points=self.points)

    def overlay(self) -> dict[str, object]:
        """The track as plain data for drawing over the video in the browser.

        ``{"fps": ..., "points": [...]}``: for each frame ``null`` when no landmark was found, or
        21 entries of ``[x, y]`` (rounded to a tenth of a pixel) or ``null`` for a missing one.
        Matches ``axonhand_api::LandmarkOverlay``.
        """
        frames: list[list[list[float] | None] | None] = []
        for frame in self.points:
            xy = frame[:, :2]
            found = np.isfinite(xy).all(axis=1)
            if not found.any():
                frames.append(None)
                continue
            rounded = np.round(xy, 1).tolist()
            frames.append([rounded[i] if found[i] else None for i in range(LANDMARK_COUNT)])
        return {"fps": self.fps, "points": frames}

    def save_overlay(self, path: Path) -> None:
        """Write :meth:`overlay` as compact JSON."""
        path.write_text(json.dumps(self.overlay(), separators=(",", ":")), encoding="utf-8")

    @staticmethod
    def load(path: Path) -> LandmarkTrack:
        """Read a track written by :meth:`save`.

        Raises:
            ValueError: if the file is not a landmark track of a known version.
        """
        try:
            with np.load(path, allow_pickle=False) as data:
                if int(data["version"]) != _FORMAT_VERSION:
                    raise ValueError("unknown landmark file version")
                return LandmarkTrack(
                    fps=float(data["fps"]), points=data["points"].astype(np.float64)
                )
        except (OSError, KeyError, EOFError) as error:
            raise ValueError("not a landmark track file") from error
