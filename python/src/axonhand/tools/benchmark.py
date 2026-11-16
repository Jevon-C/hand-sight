"""Time the hand pipeline stage by stage, on the CPU and on the NPU.

Usage::

    python -m axonhand.tools.benchmark VIDEO [--models DIR] [--runs N] [--out DIR]

For each device that is available it measures (a) the whole pipeline on ``VIDEO``, split into
video decoding, palm detector and landmark stage, (b) the pure inference time of each model on a
fixed input (median and 95th percentile over ``--runs`` runs after a warm-up), and (c) the tap
analysis. Writes ``benchmark.md`` and ``benchmark.json`` to ``--out`` (default ``data/bench``).
Exit status 0 on success, 1 if the models are missing.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np
import onnxruntime as ort

from axonhand.hand.runtime import Device, LoadedModel, Model, ModelError, npu_devices, open_model
from axonhand.hand.video import TrackingError, track_video
from axonhand.hand_model import LANDMARK_MODEL, PALM_MODEL, PIPELINE_VERSION
from axonhand.tapping import AnalysisError, analyse

WARM_UP = 10


@dataclass(frozen=True)
class ModelTiming:
    """Inference time of one model on a fixed input, in milliseconds."""

    median_ms: float
    p95_ms: float
    file_mb: float


@dataclass(frozen=True)
class DeviceReport:
    """Everything measured with the models on one device."""

    device: str
    palm: ModelTiming
    landmark: ModelTiming
    frames: int
    decode_ms_per_frame: float
    detect_ms_per_frame: float
    landmark_ms_per_frame: float
    total_ms_per_frame: float
    fps: float
    palm_runs: int
    analysis_ms: float | None
    notes: tuple[str, ...]


def inference_timing(model: LoadedModel, path: Path, runs: int) -> ModelTiming:
    """Time ``model`` on a zero input of its own shape."""
    shape = [d if isinstance(d, int) and d > 0 else 1 for d in model.session.get_inputs()[0].shape]
    blob = np.zeros(shape, dtype=np.float32)
    for _ in range(WARM_UP):
        model.run(blob)
    times = []
    for _ in range(runs):
        started = time.perf_counter()
        model.run(blob)
        times.append((time.perf_counter() - started) * 1000.0)
    return ModelTiming(
        median_ms=float(np.median(times)),
        p95_ms=float(np.percentile(times, 95)),
        file_mb=path.stat().st_size / 1e6,
    )


@dataclass(frozen=True)
class PipelineTiming:
    """The whole pipeline over one video."""

    frames: int
    decode_ms_per_frame: float
    detect_ms_per_frame: float
    landmark_ms_per_frame: float
    total_ms_per_frame: float
    palm_runs: int
    analysis_ms: float | None


def time_pipeline(video: Path, palm: Model, landmarks: Model) -> PipelineTiming:
    """Run the tracker over ``video`` and the tap analysis over its track, timing each part.

    Raises:
        TrackingError: if the video cannot be tracked.
    """
    started = time.perf_counter()
    track, tracker = track_video(video, palm, landmarks)
    wall = time.perf_counter() - started
    analysis_ms: float | None
    try:
        started = time.perf_counter()
        analyse(track)
        analysis_ms = (time.perf_counter() - started) * 1000.0
    except AnalysisError:
        analysis_ms = None
    frames = track.frames
    per = {k: 1000.0 * v / frames for k, v in tracker.timing.items()}
    return PipelineTiming(
        frames=frames,
        decode_ms_per_frame=per["decode"],
        detect_ms_per_frame=per["detect"],
        landmark_ms_per_frame=per["landmark"],
        total_ms_per_frame=1000.0 * wall / frames,
        palm_runs=tracker.detections,
        analysis_ms=analysis_ms,
    )


def measure(video: Path, models: Path, device: Device, runs: int) -> DeviceReport:
    """Measure the pipeline with both models on ``device``.

    Raises:
        ModelError: if a model cannot be opened on that device.
        TrackingError: if the video cannot be tracked.
    """
    palm = open_model(models / PALM_MODEL, device)
    landmarks = open_model(models / LANDMARK_MODEL, device)
    palm_timing = inference_timing(palm, models / PALM_MODEL, runs)
    landmark_timing = inference_timing(landmarks, models / LANDMARK_MODEL, runs)
    pipeline = time_pipeline(video, palm, landmarks)
    return DeviceReport(
        device=f"{palm.device}/{landmarks.device}",
        palm=palm_timing,
        landmark=landmark_timing,
        frames=pipeline.frames,
        decode_ms_per_frame=pipeline.decode_ms_per_frame,
        detect_ms_per_frame=pipeline.detect_ms_per_frame,
        landmark_ms_per_frame=pipeline.landmark_ms_per_frame,
        total_ms_per_frame=pipeline.total_ms_per_frame,
        fps=1000.0 / pipeline.total_ms_per_frame,
        palm_runs=pipeline.palm_runs,
        analysis_ms=pipeline.analysis_ms,
        notes=tuple(m.note for m in (palm, landmarks) if m.note),
    )


def _windows_peak_working_set_mb() -> float | None:
    """Peak working set of this process in MB, from the Windows process API."""
    if sys.platform != "win32":
        return None
    import ctypes
    from ctypes import wintypes

    class Counters(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
        ]

    counters = Counters()
    counters.cb = ctypes.sizeof(Counters)
    kernel = ctypes.WinDLL("kernel32")
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    psapi = ctypes.WinDLL("psapi")
    psapi.GetProcessMemoryInfo.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(Counters),
        wintypes.DWORD,
    ]
    ok = psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.cb)
    return float(counters.PeakWorkingSetSize) / 1e6 if ok else None


def peak_memory_mb() -> float | None:
    """Peak memory of this process in MB, if the platform can tell."""
    try:
        if sys.platform == "win32":
            return _windows_peak_working_set_mb()
        import resource

        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        return float(peak) / (1e6 if sys.platform == "darwin" else 1e3)
    except (ImportError, OSError, AttributeError):
        return None


def environment() -> dict[str, str]:
    """Facts about the machine and software, so a table can be reproduced."""
    return {
        "machine": platform.machine(),
        "system": f"{platform.system()} {platform.release()}",
        "python": platform.python_version(),
        "onnxruntime": ort.__version__,
        "cpu_count": str(os.cpu_count()),
        "npu_devices": str(len(npu_devices())),
        "pipeline": PIPELINE_VERSION,
    }


def markdown(
    video: str, reports: list[DeviceReport], env: dict[str, str], memory: float | None
) -> str:
    """The table as Markdown."""
    lines = [
        "# Speed of the hand pipeline",
        "",
        f"Clip: {video}. " + "; ".join(f"{k} {v}" for k, v in env.items()) + ".",
        "",
        "## Whole pipeline (milliseconds per frame)",
        "",
        "| Devices (palm/landmark) | Decode | Palm detector | Landmark stage | Total "
        "| Frames per second | Tap analysis (ms, whole clip) |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in reports:
        analysis = f"{r.analysis_ms:.1f}" if r.analysis_ms is not None else "n/a"
        lines.append(
            f"| {r.device} | {r.decode_ms_per_frame:.2f} | {r.detect_ms_per_frame:.3f} "
            f"| {r.landmark_ms_per_frame:.2f} | {r.total_ms_per_frame:.2f} | {r.fps:.0f} "
            f"| {analysis} |"
        )
    lines += [
        "",
        "The palm detector runs only when the hand is lost, so its per-frame share is small; "
        "the landmark stage includes cropping the hand.",
        "",
        "## Each model on its own (milliseconds per inference)",
        "",
        "| Devices | Palm median | Palm 95th | Landmark median | Landmark 95th "
        "| Palm file MB | Landmark file MB |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in reports:
        lines.append(
            f"| {r.device} | {r.palm.median_ms:.2f} | {r.palm.p95_ms:.2f} "
            f"| {r.landmark.median_ms:.2f} | {r.landmark.p95_ms:.2f} "
            f"| {r.palm.file_mb:.1f} | {r.landmark.file_mb:.1f} |"
        )
    if memory is not None:
        lines += ["", f"Peak memory of the benchmark process: {memory:.0f} MB."]
    notes = sorted({n for r in reports for n in r.notes})
    if notes:
        lines += ["", "Notes: " + " ".join(notes)]
    lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument(
        "--models", type=Path, default=Path(os.environ.get("AXONHAND_MODELS", "models"))
    )
    parser.add_argument("--runs", type=int, default=100)
    parser.add_argument("--out", type=Path, default=Path("data/bench"))
    args = parser.parse_args(argv)
    devices: list[Device] = ["cpu"]
    if npu_devices():
        devices.append("npu")
    reports: list[DeviceReport] = []
    for device in devices:
        try:
            report = measure(args.video, args.models, device, args.runs)
        except (ModelError, TrackingError) as error:
            print(f"{device}: {error}", file=sys.stderr)
            if device == "cpu":
                return 1
            continue
        reports.append(report)
        print(
            f"{report.device}: {report.total_ms_per_frame:.2f} ms/frame "
            f"({report.fps:.0f} fps); landmark model {report.landmark.median_ms:.2f} ms"
        )
    memory = peak_memory_mb()
    env = environment()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "benchmark.md").write_text(
        markdown(args.video.name, reports, env, memory), encoding="utf-8"
    )
    (args.out / "benchmark.json").write_text(
        json.dumps(
            {"environment": env, "peak_memory_mb": memory, "devices": [asdict(r) for r in reports]},
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"written to {args.out / 'benchmark.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
