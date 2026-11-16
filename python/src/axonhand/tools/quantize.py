"""Make an int8 version of the landmark model and measure what it costs in accuracy.

Usage::

    python -m axonhand.tools.quantize VIDEO [--models DIR] [--activation uint8|uint16]
                                            [--frames N] [--out DIR]

The video must show a hand. The float pipeline is run over it and the landmark model's inputs are
recorded; the first half of the recorded inputs calibrates the quantisation, the second half is
held out and used to measure the error. Then the whole pipeline is run again with the quantised
model and the tap analysis of the two is compared. Writes the quantised model and
``quantisation.md`` to ``--out`` (default ``data/quant``). Exit status 0 on success, 1 on a
missing model or an unusable video.

The held-out half comes from the same clip, so the result is optimistic: it shows the
quantisation does not break what it sees, not how it behaves on other hands.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from numpy.typing import NDArray
from onnxruntime.quantization import (
    CalibrationDataReader,
    QuantFormat,
    QuantType,
    quantize_static,
)

from axonhand.hand.runtime import Device, Model, ModelError, npu_devices, open_model
from axonhand.hand.video import TrackingError, track_video
from axonhand.hand_model import LANDMARK_MODEL, PALM_MODEL
from axonhand.landmarks import MIDDLE_MCP, WRIST, LandmarkTrack
from axonhand.tapping import AnalysisError, analyse

Blob = NDArray[np.float32]

MAX_MEDIAN_ERROR = 0.02
MAX_P95_ERROR = 0.05
"""Proposed limits on the landmark error in palm lengths (to be confirmed, docs/EVALUATION.md)."""
QUANTISED_SUFFIX = "_int8.onnx"


class RecordingModel:
    """Passes every call to ``model`` and keeps a copy of each input."""

    def __init__(self, model: Model) -> None:
        self._model = model
        self.inputs: list[Blob] = []

    def run(self, blob: Blob) -> list[NDArray[np.float32]]:
        """Run the wrapped model and remember what it was given."""
        self.inputs.append(blob.copy())
        return self._model.run(blob)


class _Reader(CalibrationDataReader):  # type: ignore[misc]
    def __init__(self, name: str, blobs: list[Blob]) -> None:
        self._name = name
        self._iterator = iter(blobs)

    def get_next(self) -> dict[str, Blob] | None:
        blob = next(self._iterator, None)
        return None if blob is None else {self._name: blob}


def quantise(
    source: Path, target: Path, input_name: str, blobs: list[Blob], activation: str
) -> None:
    """Write an int8 QDQ model made from ``source``, calibrated on ``blobs``.

    Weights are 8-bit; activations are 8-bit, or 16-bit when ``activation`` is ``"uint16"``.
    """
    sixteen = activation == "uint16"
    quantize_static(
        source,
        target,
        _Reader(input_name, blobs),
        quant_format=QuantFormat.QDQ,
        per_channel=True,
        activation_type=QuantType.QUInt16 if sixteen else QuantType.QUInt8,
        weight_type=QuantType.QUInt8,
        extra_options={"UseQDQContribOps": sixteen},
    )


def split_blobs(blobs: list[Blob], limit: int) -> tuple[list[Blob], list[Blob]]:
    """At most ``limit`` inputs, evenly spread: the first half calibrates, the rest is held out."""
    if len(blobs) < 4:
        raise ValueError("at least four landmark inputs are needed")
    picks = np.linspace(0, len(blobs) - 1, min(limit, len(blobs))).round().astype(int)
    chosen = [blobs[i] for i in np.unique(picks)]
    half = len(chosen) // 2
    return chosen[:half], chosen[half:]


@dataclass(frozen=True)
class LandmarkError:
    """Landmark error of one model against the float one, in palm lengths."""

    median: float
    p95: float
    worst: float
    score_difference: float
    """Largest difference in the presence score."""


def landmark_error(reference: Model, candidate: Model, blobs: list[Blob]) -> LandmarkError:
    """How far ``candidate`` moves the landmarks from ``reference`` on the same inputs."""
    errors: list[float] = []
    score = 0.0
    for blob in blobs:
        ref = reference.run(blob)
        cand = candidate.run(blob)
        a = ref[0].reshape(21, 3)[:, :2]
        b = cand[0].reshape(21, 3)[:, :2]
        palm = float(np.linalg.norm(a[WRIST] - a[MIDDLE_MCP]))
        errors.extend((np.linalg.norm(a - b, axis=1) / max(palm, 1e-6)).tolist())
        score = max(score, abs(float(ref[1].reshape(-1)[0]) - float(cand[1].reshape(-1)[0])))
    arr = np.asarray(errors)
    return LandmarkError(
        float(np.median(arr)), float(np.percentile(arr, 95)), float(arr.max()), score
    )


@dataclass(frozen=True)
class TapComparison:
    """The tap analysis of the quantised pipeline against the float one on the same video."""

    float_taps: int
    quantised_taps: int
    contact_diff_ms_max: float
    rate_diff_percent: float


def compare_tracks(float_track: LandmarkTrack, quantised_track: LandmarkTrack) -> TapComparison:
    """Compare the tap analysis of two tracks of the same video.

    Raises:
        AnalysisError: if either track cannot be analysed.
    """
    a = analyse(float_track)
    b = analyse(quantised_track)
    count = min(a.contact_times.size, b.contact_times.size)
    diff = np.abs(a.contact_times[:count] - b.contact_times[:count]) * 1000.0
    return TapComparison(
        a.tap_count,
        b.tap_count,
        float(diff.max()) if count else float("nan"),
        100.0 * (b.tap_rate_hz - a.tap_rate_hz) / a.tap_rate_hz,
    )


def verdict(error: LandmarkError, taps: TapComparison | None, fps: float) -> list[tuple[str, bool]]:
    """Each proposed limit and whether the measured quantisation meets it."""
    out = [
        (
            f"median landmark error at most {MAX_MEDIAN_ERROR} palm lengths",
            error.median <= MAX_MEDIAN_ERROR,
        ),
        (f"95th percentile at most {MAX_P95_ERROR} palm lengths", error.p95 <= MAX_P95_ERROR),
    ]
    if taps is not None:
        out.append(("same number of taps", taps.float_taps == taps.quantised_taps))
        out.append(("contact times within one frame", taps.contact_diff_ms_max <= 1000.0 / fps))
    return out


def timing_ms(model: Model, blob: Blob, runs: int = 50) -> float:
    """Median inference time in milliseconds."""
    for _ in range(5):
        model.run(blob)
    times = []
    for _ in range(runs):
        started = time.perf_counter()
        model.run(blob)
        times.append((time.perf_counter() - started) * 1000.0)
    return float(np.median(times))


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument(
        "--models", type=Path, default=Path(os.environ.get("AXONHAND_MODELS", "models"))
    )
    parser.add_argument("--activation", choices=["uint8", "uint16"], default="uint8")
    parser.add_argument("--frames", type=int, default=120)
    parser.add_argument("--out", type=Path, default=Path("data/quant"))
    args = parser.parse_args(argv)
    source = args.models / LANDMARK_MODEL
    try:
        palm = open_model(args.models / PALM_MODEL, "auto")
        floating = open_model(source, "cpu")
    except ModelError as error:
        print(f"model unavailable: {error}", file=sys.stderr)
        return 1
    recorder = RecordingModel(floating)
    try:
        float_track, _ = track_video(args.video, palm, recorder)
        calibration, held_out = split_blobs(recorder.inputs, args.frames)
    except (TrackingError, ValueError) as error:
        print(f"cannot use the video: {error}", file=sys.stderr)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)
    target = args.out / (source.stem + QUANTISED_SUFFIX)
    quantise(source, target, floating.input_name, calibration, args.activation)
    print(f"quantised model written to {target} ({target.stat().st_size / 1e6:.1f} MB)")
    lines = [
        "# Int8 landmark model",
        "",
        f"Activations {args.activation}, weights uint8, per channel. Calibrated on "
        f"{len(calibration)} inputs, error measured on {len(held_out)} held-out inputs of the "
        f"same clip ({args.video.name}), so it is optimistic. File "
        f"{source.stat().st_size / 1e6:.1f} MB float, {target.stat().st_size / 1e6:.1f} MB int8.",
        "",
        "| Device | Median error | 95th pct | Worst | Score diff | Taps float/int8 "
        "| Contact diff ms | Rate diff % | Inference ms (float CPU / int8) |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    devices: list[Device] = ["cpu", "npu"] if npu_devices() else ["cpu"]
    fps = float_track.fps
    ok_all = True
    for device in devices:
        try:
            quantised = open_model(target, device)
        except ModelError as failure:
            lines.append(f"| {device} | could not load: {failure} |")
            continue
        measured = landmark_error(floating, quantised, held_out)
        try:
            quant_track, _ = track_video(args.video, palm, quantised)
            taps: TapComparison | None = compare_tracks(float_track, quant_track)
        except (TrackingError, AnalysisError):
            taps = None
        speed = f"{timing_ms(floating, held_out[0]):.2f} / {timing_ms(quantised, held_out[0]):.2f}"
        tap_cols = (
            f"{taps.float_taps}/{taps.quantised_taps} | {taps.contact_diff_ms_max:.0f} | "
            f"{taps.rate_diff_percent:+.2f}"
            if taps
            else "n/a | n/a | n/a"
        )
        lines.append(
            f"| {quantised.device} | {measured.median:.4f} | {measured.p95:.4f} "
            f"| {measured.worst:.4f} | {measured.score_difference:.3f} | {tap_cols} | {speed} |"
        )
        checks = verdict(measured, taps, fps)
        ok_all = ok_all and all(ok for _, ok in checks)
        lines += ["", f"Against the proposed limits on {quantised.device}:", ""]
        lines += [f"- {'MET' if ok else 'NOT MET'}: {name}" for name, ok in checks]
        lines.append("")
        print(
            f"{quantised.device}: median error {measured.median:.4f} palm lengths, "
            f"p95 {measured.p95:.4f}"
        )
    lines.append(
        "Use the int8 model only if every limit is met on the device it will run on, and "
        "only if the speed column shows a gain worth having."
        if not ok_all
        else "All proposed limits are met on this clip."
    )
    (args.out / "quantisation.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"report written to {args.out / 'quantisation.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
