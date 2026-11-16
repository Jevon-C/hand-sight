"""Find the hand in every frame of a video with the real models.

Usage::

    python -m axonhand.hand_model VIDEO OUT.npz [--models DIR] [--device auto|cpu|npu]

Writes the landmark track to ``OUT.npz`` and the same landmarks for drawing over the video to
``OUT.json``, and prints one line saying how it ran. Exit status 0 on
success, 2 when the video cannot be used (the reason is on stderr, written for a clinician), and
1 for a fault in the models or the software.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path
from typing import cast

from axonhand.hand.runtime import Device, ModelError, open_model
from axonhand.hand.video import TrackingError, track_video

PALM_MODEL = "palm_detection_mediapipe_2023feb.onnx"
LANDMARK_MODEL = "handpose_estimation_mediapipe_2023feb.onnx"
PIPELINE_VERSION = "mediapipe-two-stage-1"
# The model runtime may write its own warnings to stderr. The server shows the person only the
# line that starts with this marker.
MESSAGE_PREFIX = "axonhand-message: "
NPU_UNAVAILABLE = "The NPU could not be used on this computer. Run the analysis on the CPU instead."


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument("output", type=Path, help="where to write the landmark track (.npz)")
    parser.add_argument(
        "--models",
        type=Path,
        default=Path(os.environ.get("AXONHAND_MODELS", "models")),
        help="folder holding the two ONNX files (default: $AXONHAND_MODELS or ./models)",
    )
    parser.add_argument("--device", choices=["auto", "cpu", "npu"], default="auto")
    args = parser.parse_args(argv)
    try:
        device = cast(Device, args.device)
        palm = open_model(args.models / PALM_MODEL, device)
        landmarks = open_model(args.models / LANDMARK_MODEL, device)
    except ModelError as error:
        if args.device == "npu" and "model file not found" not in str(error):
            # The NPU was demanded: the person can choose the CPU, so this is not a fault.
            print(f"NPU: {error}", file=sys.stderr)
            print(f"{MESSAGE_PREFIX}{NPU_UNAVAILABLE}", file=sys.stderr)
            return 2
        print(f"hand model unavailable: {error}", file=sys.stderr)
        return 1
    started = time.perf_counter()
    try:
        track, tracker = track_video(args.video, palm, landmarks)
    except TrackingError as error:
        print(f"{MESSAGE_PREFIX}{error}", file=sys.stderr)
        return 2
    except ModelError as error:
        print(f"hand model failed: {error}", file=sys.stderr)
        return 1
    track.save(args.output)
    track.save_overlay(args.output.with_suffix(".json"))
    seconds = time.perf_counter() - started
    print(
        f"{track.frames} frames in {seconds:.1f} s; palm detector on {palm.device} "
        f"({tracker.detections} runs), landmark model on {landmarks.device}"
    )
    print(
        "time in seconds: "
        + ", ".join(f"{name} {value:.2f}" for name, value in tracker.timing.items())
    )
    for name, model in (("palm detector", palm), ("landmark model", landmarks)):
        if model.note:
            print(f"note: {name}: {model.note}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
