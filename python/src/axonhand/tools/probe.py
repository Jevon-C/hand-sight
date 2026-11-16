"""Read basic facts about a video file: size, frame rate, length and codec.

The server runs this as a separate process::

    python -m axonhand.tools.probe path/to/video.mp4

On success it prints one JSON object and exits with 0. On failure it prints
``{"error": "..."}`` and exits with 2, so the caller can show a plain message. Only the file's
header and stream index are read; no frames are decoded.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

import av
from av.error import FFmpegError


class ProbeError(Exception):
    """The file could not be read as a video."""


@dataclass(frozen=True)
class VideoProbe:
    """Facts about the first video stream in a file."""

    width: int
    height: int
    fps: float
    duration_seconds: float
    frame_count: int
    codec: str


def probe(path: Path) -> VideoProbe:
    """Describe the first video stream of ``path``.

    Raises:
        ProbeError: if the file is missing, unreadable, has no video stream, or reports a
            frame rate or length that is not a positive number.
    """
    if not path.is_file():
        raise ProbeError(f"no such file: {path.name}")
    try:
        with av.open(str(path)) as container:
            streams = container.streams.video
            if not streams:
                raise ProbeError("the file has no video stream")
            stream = streams[0]
            rate = stream.average_rate
            fps = float(rate) if rate is not None else 0.0
            if stream.duration is not None and stream.time_base is not None:
                duration = float(stream.duration * stream.time_base)
            elif container.duration is not None:
                duration = container.duration / av.time_base
            else:
                duration = 0.0
            frames = stream.frames or round(duration * fps)
            result = VideoProbe(
                width=int(stream.codec_context.width),
                height=int(stream.codec_context.height),
                fps=fps,
                duration_seconds=duration,
                frame_count=int(frames),
                codec=str(stream.codec_context.name),
            )
    except FFmpegError as error:
        raise ProbeError("the file is not a readable video") from error
    if result.fps <= 0 or result.duration_seconds <= 0 or result.width <= 0 or result.height <= 0:
        raise ProbeError("the video reports no usable size, frame rate or length")
    return result


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path, help="path to a video file")
    args = parser.parse_args(argv)
    try:
        print(json.dumps(asdict(probe(args.video))))
    except ProbeError as error:
        print(json.dumps({"error": str(error)}))
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
