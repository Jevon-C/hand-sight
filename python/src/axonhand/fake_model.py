"""A stand-in hand model that finds coloured dots, for development and tests.

It reads videos drawn by :func:`axonhand.synthetic.render_video` and returns the same kind of
landmark track a real model would. It lets the whole path from an uploaded video to a saved
result run before the real model is wired in (phase 5). It finds only four landmarks (wrist,
middle knuckle, thumb tip, index tip) and is of no use on a real recording.

Because the thumb dot can be partly hidden by the index dot when the fingers touch, the closed
position is measured with an error of up to one dot radius (about 0.06 palm lengths at the
default drawing size). The widest openings, which carry the measures, are not affected.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import av
import numpy as np
from av.error import FFmpegError
from numpy.typing import NDArray

from axonhand.landmarks import LANDMARK_COUNT, LandmarkTrack
from axonhand.synthetic import MARKER_COLOURS

_COLOUR_DISTANCE = 90.0
_MIN_PIXELS = 4
_SCALE_PER_HEIGHT = 0.35  # must match render_video
_ORIGIN = (0.5, 0.9)  # must match render_video, as fractions of width and height


class TrackingError(Exception):
    """The video could not be tracked. The message is written for a clinician."""


def track_markers(video: Path) -> LandmarkTrack:
    """Find the coloured marker dots in every frame of ``video``.

    Raises:
        TrackingError: if the file cannot be read as a video.
    """
    return _track(video)[0]


def pixel_track(track: LandmarkTrack, width: int, height: int) -> LandmarkTrack:
    """``track`` (in the drawing's own units) as pixel positions in a ``width`` by ``height``
    video, the form used to draw landmarks over the video."""
    scale = _SCALE_PER_HEIGHT * height
    pixels = track.points.copy()
    pixels[:, :, 0] = track.points[:, :, 0] * scale + _ORIGIN[0] * width
    pixels[:, :, 1] = _ORIGIN[1] * height - track.points[:, :, 1] * scale
    return LandmarkTrack(fps=track.fps, points=pixels)


def _track(video: Path) -> tuple[LandmarkTrack, tuple[int, int]]:
    try:
        with av.open(str(video)) as container:
            stream = container.streams.video[0]
            size = (int(stream.width), int(stream.height))
            rate = stream.average_rate
            fps = float(rate) if rate is not None else 0.0
            frames = [
                _find_dots(np.asarray(frame.to_ndarray(format="rgb24"), dtype=np.uint8))
                for frame in container.decode(stream)
            ]
    except (FFmpegError, IndexError) as error:
        raise TrackingError("The video could not be read.") from error
    if not frames or fps <= 0:
        raise TrackingError("The video has no frames.")
    return LandmarkTrack(fps=fps, points=np.stack(frames)), size


def _find_dots(image: NDArray[np.uint8]) -> NDArray[np.float64]:
    height, width = image.shape[:2]
    points = np.full((LANDMARK_COUNT, 3), np.nan)
    pixels = image.astype(np.float64)
    yy, xx = np.mgrid[0:height, 0:width]
    scale = _SCALE_PER_HEIGHT * height
    for landmark, colour in MARKER_COLOURS.items():
        distance = np.linalg.norm(pixels - np.array(colour, dtype=np.float64), axis=2)
        mask = distance < _COLOUR_DISTANCE
        if int(mask.sum()) < _MIN_PIXELS:
            continue
        cx, cy = float(xx[mask].mean()), float(yy[mask].mean())
        points[landmark, 0] = (cx - _ORIGIN[0] * width) / scale
        points[landmark, 1] = (_ORIGIN[1] * height - cy) / scale
        points[landmark, 2] = 0.0
    return points


def main(argv: list[str] | None = None) -> int:
    """Command line entry point: ``python -m axonhand.fake_model VIDEO OUT.npz``.

    Also writes ``OUT.json``, the landmarks in pixels for drawing over the video.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument("output", type=Path, help="where to write the landmark track (.npz)")
    args = parser.parse_args(argv)
    try:
        track, (width, height) = _track(args.video)
        track.save(args.output)
        pixel_track(track, width, height).save_overlay(args.output.with_suffix(".json"))
    except TrackingError as error:
        print(str(error), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
