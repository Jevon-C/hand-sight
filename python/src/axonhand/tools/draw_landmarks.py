"""Draw hand landmarks on a video, to see what the hand model found.

Usage::

    python -m axonhand.tools.draw_landmarks VIDEO LANDMARKS.npz SHEET.png [--video-out OUT.mp4]

Writes a contact sheet (a grid of evenly spaced frames with the landmarks and finger bones drawn)
and, if asked, the whole video with the drawing on it. Frames where no hand was found are marked.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import av
import numpy as np
from numpy.typing import NDArray
from PIL import Image, ImageDraw

from axonhand.hand.video import TrackingError, read_frames
from axonhand.landmarks import LandmarkTrack

# Bones of the MediaPipe hand: pairs of landmark numbers.
BONES = (
    (0, 1), (1, 2), (2, 3), (3, 4),
    (0, 5), (5, 6), (6, 7), (7, 8),
    (5, 9), (9, 10), (10, 11), (11, 12),
    (9, 13), (13, 14), (14, 15), (15, 16),
    (13, 17), (0, 17), (17, 18), (18, 19), (19, 20),
)  # fmt: skip
HIGHLIGHT = {4: (255, 60, 60), 8: (60, 120, 255)}  # thumb tip red, index tip blue


def draw(frame: NDArray[np.uint8], points: NDArray[np.float64]) -> Image.Image:
    """The frame with the hand drawn on it, or a "no hand" note if every landmark is missing."""
    image = Image.fromarray(frame)
    pen = ImageDraw.Draw(image)
    if np.isnan(points[:, :2]).all():
        pen.text((8, 8), "no hand found", fill=(255, 255, 0))
        return image
    radius = max(2, round(min(image.size) / 120))
    for a, b in BONES:
        pa, pb = points[a, :2], points[b, :2]
        if np.isfinite(pa).all() and np.isfinite(pb).all():
            pen.line([tuple(pa), tuple(pb)], fill=(255, 255, 255), width=max(1, radius // 2))
    for i, (x, y) in enumerate(points[:, :2]):
        if np.isfinite(x) and np.isfinite(y):
            colour = HIGHLIGHT.get(i, (60, 220, 120))
            pen.ellipse([x - radius, y - radius, x + radius, y + radius], fill=colour)
    return image


def contact_sheet(images: list[Image.Image], columns: int = 3, width: int = 480) -> Image.Image:
    """Arrange frames in a grid, each scaled to ``width`` pixels wide."""
    scaled = [im.resize((width, round(im.height * width / im.width))) for im in images]
    rows = -(-len(scaled) // columns)
    height = max(im.height for im in scaled)
    sheet = Image.new("RGB", (columns * width, rows * height), (20, 20, 20))
    for index, im in enumerate(scaled):
        sheet.paste(im, ((index % columns) * width, (index // columns) * height))
    return sheet


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument("landmarks", type=Path)
    parser.add_argument("sheet", type=Path, help="where to write the contact sheet (.png)")
    parser.add_argument("--video-out", type=Path, help="also write the full drawn video (.mp4)")
    parser.add_argument("--frames", type=int, default=9, help="frames in the contact sheet")
    args = parser.parse_args(argv)
    try:
        track = LandmarkTrack.load(args.landmarks)
        fps, frames = read_frames(args.video)
    except (ValueError, TrackingError) as error:
        print(str(error), file=sys.stderr)
        return 2
    wanted = set(np.linspace(0, track.frames - 1, args.frames).astype(int).tolist())
    picked: list[Image.Image] = []
    out = av.open(str(args.video_out), mode="w") if args.video_out else None
    stream = None
    for index, frame in enumerate(frames):
        if index >= track.frames:
            break
        if index not in wanted and out is None:
            continue
        drawn = draw(frame, track.points[index])
        if index in wanted:
            picked.append(drawn)
        if out is not None:
            if stream is None:
                stream = out.add_stream("libx264", rate=round(fps))
                stream.width, stream.height = drawn.width, drawn.height
                stream.pix_fmt = "yuv420p"
            for packet in stream.encode(
                av.VideoFrame.from_ndarray(np.asarray(drawn), format="rgb24")
            ):
                out.mux(packet)
    if out is not None and stream is not None:
        for packet in stream.encode():
            out.mux(packet)
        out.close()
    contact_sheet(picked).save(args.sheet)
    found = int((~np.isnan(track.points[:, :, :2]).all(axis=(1, 2))).sum())
    print(f"hand found in {found} of {track.frames} frames; sheet written to {args.sheet}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
