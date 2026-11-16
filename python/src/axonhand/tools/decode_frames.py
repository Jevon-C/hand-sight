"""Decode a video to raw frames on standard output, for the Rust hand tracker.

Usage: ``python -m axonhand.tools.decode_frames VIDEO``

Writes one text line ``AXONFRAMES 1 WIDTH HEIGHT FPS`` and then every frame as WIDTH * HEIGHT * 3
bytes of RGB, turned upright the way a person sees the video (see
:func:`axonhand.hand.video.read_frames`). Exit status 0 on success, 2 when the video cannot be
used (the reason is on stderr, written for a clinician, after the marker the server looks for), 1
for a fault. Decoding stays in Python because PyAV already reads every phone format on every
platform we use; the Rust program does the model work, which is where the time goes.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import BinaryIO

from axonhand.hand.video import TrackingError, read_frames

MESSAGE_PREFIX = "axonhand-message: "
MAGIC = "AXONFRAMES"
VERSION = 1


def decode(video: Path, out: BinaryIO) -> int:
    """Write the header and the frames of ``video`` to ``out``; return the number of frames.

    Raises:
        TrackingError: if the video cannot be read or has no frames.
    """
    fps, frames = read_frames(video)
    written = 0
    for frame in frames:
        if written == 0:
            height, width = frame.shape[:2]
            out.write(f"{MAGIC} {VERSION} {width} {height} {fps!r}\n".encode("ascii"))
        elif frame.shape[:2] != (height, width):
            raise TrackingError("The video changes size part way through.")
        out.write(frame.tobytes())
        written += 1
    if written == 0:
        raise TrackingError("The video has no frames.")
    out.flush()
    return written


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    args = parser.parse_args(argv)
    try:
        decode(args.video, sys.stdout.buffer)
    except TrackingError as error:
        print(f"{MESSAGE_PREFIX}{error}", file=sys.stderr)
        return 2
    except BrokenPipeError:
        # The tracker stopped reading; its own exit status says why.
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
