"""Run the hand pipeline over a whole video."""

from __future__ import annotations

import time
from collections.abc import Callable, Iterator
from pathlib import Path

import av
import numpy as np
from av.error import FFmpegError
from numpy.typing import NDArray

from axonhand.hand.runtime import Model
from axonhand.hand.tracker import HandTracker
from axonhand.landmarks import LANDMARK_COUNT, LandmarkTrack


class TrackingError(Exception):
    """The video could not be tracked. The message is written for a clinician."""


def upright(image: NDArray[np.uint8], rotation_degrees: float) -> NDArray[np.uint8]:
    """Turn a decoded frame the way the video's display flag says.

    ``rotation_degrees`` is the flag as PyAV reports it: positive means the picture is shown
    turned anticlockwise, which is what ``numpy.rot90`` does for a positive count.
    """
    turns = (int(round(rotation_degrees)) // 90) % 4
    return np.ascontiguousarray(np.rot90(image, turns)) if turns else image


def read_frames(path: Path) -> tuple[float, Iterator[NDArray[np.uint8]]]:
    """Open a video; return its frame rate and an iterator of upright RGB frames.

    Phone videos often carry a rotation flag instead of rotated pixels; it is applied here, so
    the hand model always sees the picture the way a person would.

    Raises:
        TrackingError: if the file cannot be read as a video.
    """
    try:
        container = av.open(str(path))
        stream = container.streams.video[0]
    except (FFmpegError, IndexError, OSError) as error:
        raise TrackingError("The video could not be read.") from error
    rate = stream.average_rate
    fps = float(rate) if rate is not None else 0.0
    if fps <= 0:
        container.close()
        raise TrackingError("The video does not say how many frames per second it has.")

    def frames() -> Iterator[NDArray[np.uint8]]:
        try:
            for frame in container.decode(stream):
                image = np.asarray(frame.to_ndarray(format="rgb24"), dtype=np.uint8)
                yield upright(image, float(getattr(frame, "rotation", 0) or 0))
        except FFmpegError as error:
            raise TrackingError("The video could not be read to the end.") from error
        finally:
            container.close()

    return fps, frames()


def track_video(
    path: Path,
    palm: Model,
    landmarks: Model,
    progress: Callable[[int], None] | None = None,
) -> tuple[LandmarkTrack, HandTracker]:
    """Find the hand in every frame of ``path``.

    A frame where no hand is found has every landmark NaN. ``progress`` is called with the number
    of frames done after each frame.

    Raises:
        TrackingError: if the video cannot be read, is empty, or no hand is found in any frame.
    """
    fps, frames = read_frames(path)
    tracker = HandTracker(palm, landmarks)
    rows: list[NDArray[np.float64]] = []
    found = 0
    iterator = iter(frames)
    index = 0
    while True:
        started = time.perf_counter()
        frame = next(iterator, None)
        tracker.timing["decode"] += time.perf_counter() - started
        if frame is None:
            break
        index += 1
        result = tracker.process(frame)
        if result is None:
            rows.append(np.full((LANDMARK_COUNT, 3), np.nan))
        else:
            rows.append(result.points)
            found += 1
        if progress is not None:
            progress(index)
    if not rows:
        raise TrackingError("The video has no frames.")
    if found == 0:
        raise TrackingError("No hand was found in this video. Check that the hand is in view.")
    return LandmarkTrack(fps=fps, points=np.stack(rows)), tracker
