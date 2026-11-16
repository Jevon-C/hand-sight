"""Write the numbers the Rust hand crate is checked against.

The Rust crate (``crates/hand``) must give the same answers as the Python pipeline. This tool runs
the Python code on fixed inputs and saves the inputs and answers as JSON; ``crates/hand/tests/
parity.rs`` reads the file and compares. With ``--models`` it also records what the two ONNX
models return for fixed inputs, so the Rust runtime can be compared too.

Run, from the repository root:
``python -m axonhand.tools.parity_fixture crates/hand/tests/fixtures/parity.json --models models``

With ``--clip VIDEO --frames N OUTFOLDER`` it instead exports frames and Python's landmarks for a
real clip, so the Rust tracker can be compared end to end (see docs/RUST-PORT.md).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image

from axonhand.hand import rois

_SEED = 20261113


def _round_trip(value: dict[str, Any]) -> dict[str, Any]:
    """Make numpy numbers into plain JSON values."""
    plain: dict[str, Any] = json.loads(json.dumps(value, default=lambda o: np.asarray(o).tolist()))
    return plain


def _image_cases(rng: np.random.Generator) -> dict[str, Any]:
    """A smooth-ish picture, resized and cropped by Pillow."""
    width, height = 120, 80
    ys, xs = np.mgrid[0:height, 0:width]
    base = np.stack([xs * 2, ys * 3, (xs + ys)], axis=2) % 256
    noise = rng.integers(0, 40, size=(height, width, 3))
    pixels = np.clip(base + noise, 0, 255).astype(np.uint8)
    image = Image.fromarray(pixels)
    matrix = rois.crop_matrix(rois.Rect(cx=70.0, cy=38.0, size=90.0, rotation=0.6), 32)
    crop = image.transform(
        (32, 32), Image.Transform.AFFINE, matrix, resample=Image.Resampling.BILINEAR
    )
    far = rois.crop_matrix(rois.Rect(cx=5.0, cy=70.0, size=60.0, rotation=-1.2), 32)
    edge = image.transform(
        (32, 32), Image.Transform.AFFINE, far, resample=Image.Resampling.BILINEAR
    )
    return {
        "width": width,
        "height": height,
        "pixels": pixels.reshape(-1).tolist(),
        "resize": [
            {
                "to": [96, 64],
                "out": np.asarray(image.resize((96, 64), Image.Resampling.BILINEAR))
                .reshape(-1)
                .tolist(),
            },
            {
                "to": [37, 25],
                "out": np.asarray(image.resize((37, 25), Image.Resampling.BILINEAR))
                .reshape(-1)
                .tolist(),
            },
            {
                "to": [200, 130],
                "out": np.asarray(image.resize((200, 130), Image.Resampling.BILINEAR))
                .reshape(-1)
                .tolist(),
            },
        ],
        "affine": [
            {"matrix": list(matrix), "size": 32, "out": np.asarray(crop).reshape(-1).tolist()},
            {"matrix": list(far), "size": 32, "out": np.asarray(edge).reshape(-1).tolist()},
        ],
    }


def _decode_case(rng: np.random.Generator) -> dict[str, Any]:
    """Random detector output with a few strong palms planted in it."""
    raw = rng.normal(0.0, 6.0, size=(rois.palm_anchors().shape[0], 18)).astype(np.float32)
    raw[:, 2:4] = rng.uniform(20, 70, size=(raw.shape[0], 2)).astype(np.float32)
    logits = rng.normal(-6.0, 2.0, size=(raw.shape[0],)).astype(np.float32)
    for index, value in ((150, 4.0), (151, 3.0), (900, 2.5), (1500, 1.2)):
        logits[index] = value
    mapping, _, _ = rois.letterbox(1280, 720)
    palms = rois.decode_palms(raw, logits, mapping)
    return {
        "raw": raw.reshape(-1).tolist(),
        "logits": logits.tolist(),
        "frame": [1280, 720],
        "palms": [
            {
                "score": p.score,
                "cx": p.cx,
                "cy": p.cy,
                "width": p.width,
                "height": p.height,
                "keypoints": p.keypoints.tolist(),
                "rect": vars(rois.rect_from_palm(p)),
            }
            for p in palms
        ],
    }


def _geometry_cases(rng: np.random.Generator) -> dict[str, Any]:
    letterboxes = []
    for width, height in [(1920, 1080), (1080, 1920), (640, 480), (333, 777), (192, 192), (50, 7)]:
        mapping, new_w, new_h = rois.letterbox(width, height)
        letterboxes.append(
            {
                "size": [width, height],
                "scale": mapping.scale,
                "pad": [mapping.pad_x, mapping.pad_y],
                "resized": [new_w, new_h],
            }
        )
    hands = []
    for _ in range(6):
        points = rng.uniform(100, 500, size=(21, 3))
        rect = rois.rect_from_landmarks(points)
        crop_points = rng.uniform(0, 224, size=(21, 3))
        hands.append(
            {
                "points": points.tolist(),
                "rect": vars(rect),
                "matrix": list(rois.crop_matrix(rect)),
                "crop_points": crop_points.tolist(),
                "mapped": rois.crop_to_image(rect, crop_points).tolist(),
            }
        )
    return {"letterboxes": letterboxes, "hands": hands}


def model_input(size: int) -> np.ndarray:
    """A fixed picture made by a formula, so Rust can build the identical one without a file."""
    index = np.arange(size * size * 3, dtype=np.int64)
    return (((index * 37) % 255) / 255.0).astype(np.float32).reshape(1, size, size, 3)


def _model_cases(models: Path) -> dict[str, Any]:
    """What the two models return on the fixed picture, on the CPU (every 41st value is kept)."""
    from axonhand.hand.runtime import open_model

    out: dict[str, Any] = {}
    names = (
        ("palm_detection_mediapipe_2023feb.onnx", 192),
        ("handpose_estimation_mediapipe_2023feb.onnx", 224),
    )
    for name, size in names:
        outputs = open_model(models / name, "cpu").run(model_input(size))
        out[name] = {
            "size": size,
            "outputs": [{"len": o.size, "sample": o.reshape(-1)[::41].tolist()} for o in outputs],
        }
    return out


def build(models: Path | None) -> dict[str, Any]:
    """The whole fixture as plain data."""
    rng = np.random.default_rng(_SEED)
    data: dict[str, Any] = {
        "anchors": {
            "count": int(rois.palm_anchors().shape[0]),
            "sample": rois.palm_anchors()[::97].tolist(),
        },
        "geometry": _geometry_cases(rng),
        "decode": _decode_case(rng),
        "image": _image_cases(rng),
    }
    if models is not None:
        data["models"] = _model_cases(models)
    return _round_trip(data)


def export_clip(video: Path, models: Path, out: Path, frames: int) -> int:
    """Save the first ``frames`` frames of a video as PPM files and what Python found in them.

    ``out/frame_0000.ppm`` and so on, plus ``out/landmarks.json``: for each frame either ``null``
    (no hand) or 21 rows of x, y, depth. The Rust test ``clip_matches_python`` reads these.
    """
    from axonhand.hand.runtime import open_model
    from axonhand.hand.tracker import HandTracker
    from axonhand.hand.video import read_frames

    tracker = HandTracker(
        open_model(models / "palm_detection_mediapipe_2023feb.onnx", "cpu"),
        open_model(models / "handpose_estimation_mediapipe_2023feb.onnx", "cpu"),
    )
    out.mkdir(parents=True, exist_ok=True)
    _, iterator = read_frames(video)
    found: list[list[list[float]] | None] = []
    for index, frame in zip(range(frames), iterator, strict=False):
        height, width, _ = frame.shape
        header = f"P6\n{width} {height}\n255\n".encode("ascii")
        (out / f"frame_{index:04d}.ppm").write_bytes(header + frame.tobytes())
        result = tracker.process(frame)
        found.append(None if result is None else result.points.tolist())
    (out / "landmarks.json").write_text(json.dumps(found), encoding="utf-8")
    print(f"wrote {len(found)} frames to {out}")  # noqa: T201
    return 0


def main(argv: list[str] | None = None) -> int:
    """Command line entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("out", type=Path, help="JSON file to write (a folder with --clip)")
    parser.add_argument("--models", type=Path, default=None, help="folder with the two ONNX models")
    parser.add_argument("--clip", type=Path, default=None, help="export frames of this video")
    parser.add_argument("--frames", type=int, default=60, help="how many frames with --clip")
    args = parser.parse_args(argv)
    if args.clip is not None:
        if args.models is None:
            parser.error("--clip needs --models")
        return export_clip(args.clip, args.models, args.out, args.frames)
    data = build(args.models)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(data, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.2f} MB)")  # noqa: T201
    return 0


if __name__ == "__main__":
    sys.exit(main())
