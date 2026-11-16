"""Describe and time an ONNX model on the CPU and, when present, the Qualcomm NPU.

Usage::

    python -m axonhand.tools.inspect_model models/handpose.onnx --runs 50

The report is markdown, so it can be pasted into ``docs/HAND-MODEL-ASSESSMENT.md``. Nothing
here changes the model. Timings use random input, so they measure speed only, not accuracy.
"""

from __future__ import annotations

import argparse
import hashlib
import statistics
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
import onnx
import onnxruntime as ort

from axonhand.hand.runtime import accelerator_devices

# ONNX element type numbers to the numpy types we feed in.
_DTYPES: dict[int, Any] = {
    onnx.TensorProto.FLOAT: np.float32,
    onnx.TensorProto.UINT8: np.uint8,
    onnx.TensorProto.INT8: np.int8,
    onnx.TensorProto.INT32: np.int32,
    onnx.TensorProto.INT64: np.int64,
}


@dataclass(frozen=True)
class TensorInfo:
    """One model input or output."""

    name: str
    shape: tuple[int | str, ...]
    elem_type: int

    @property
    def concrete_shape(self) -> tuple[int, ...]:
        """The shape with every unknown dimension set to 1, for building test input."""
        return tuple(d if isinstance(d, int) and d > 0 else 1 for d in self.shape)


@dataclass(frozen=True)
class ModelInfo:
    """Facts read from the model file alone."""

    path: str
    size_bytes: int
    sha256: str
    opset: int
    inputs: tuple[TensorInfo, ...]
    outputs: tuple[TensorInfo, ...]


@dataclass(frozen=True)
class Timing:
    """Latency of one provider, in milliseconds."""

    label: str
    median_ms: float
    p95_ms: float
    providers: tuple[str, ...]


def _tensor_info(value: onnx.ValueInfoProto) -> TensorInfo:
    tensor_type = value.type.tensor_type
    dims: list[int | str] = []
    for dim in tensor_type.shape.dim:
        if dim.HasField("dim_value"):
            dims.append(int(dim.dim_value))
        else:
            dims.append(dim.dim_param or "?")
    return TensorInfo(value.name, tuple(dims), int(tensor_type.elem_type))


def describe(path: Path) -> ModelInfo:
    """Read size, hash, opset and tensor shapes from an ONNX file.

    Raises:
        FileNotFoundError: if ``path`` does not exist.
        onnx.onnx_cpp2py_export.checker.ValidationError: if the file is not a valid model.
    """
    data = path.read_bytes()
    model = onnx.load_from_string(data)
    onnx.checker.check_model(model)
    initialisers = {init.name for init in model.graph.initializer}
    inputs = tuple(_tensor_info(i) for i in model.graph.input if i.name not in initialisers)
    outputs = tuple(_tensor_info(o) for o in model.graph.output)
    opset = max(
        (imp.version for imp in model.opset_import if imp.domain in ("", "ai.onnx")), default=0
    )
    return ModelInfo(
        path=str(path),
        size_bytes=len(data),
        sha256=hashlib.sha256(data).hexdigest(),
        opset=opset,
        inputs=inputs,
        outputs=outputs,
    )


def _random_inputs(info: ModelInfo, seed: int = 0) -> dict[str, np.ndarray[Any, Any]]:
    rng = np.random.default_rng(seed)
    feeds: dict[str, np.ndarray[Any, Any]] = {}
    for tensor in info.inputs:
        dtype = _DTYPES.get(tensor.elem_type, np.float32)
        shape = tensor.concrete_shape
        if np.issubdtype(dtype, np.floating):
            feeds[tensor.name] = rng.standard_normal(shape).astype(dtype)
        else:
            feeds[tensor.name] = rng.integers(0, 2, size=shape).astype(dtype)
    return feeds


def time_session(session: ort.InferenceSession, info: ModelInfo, label: str, runs: int) -> Timing:
    """Time ``runs`` inferences after a short warm-up."""
    feeds = _random_inputs(info)
    for _ in range(5):
        session.run(None, feeds)
    samples: list[float] = []
    for _ in range(runs):
        start = time.perf_counter()
        session.run(None, feeds)
        samples.append((time.perf_counter() - start) * 1000.0)
    samples.sort()
    p95 = samples[min(len(samples) - 1, int(0.95 * len(samples)))]
    return Timing(label, statistics.median(samples), p95, tuple(session.get_providers()))


def benchmark(path: Path, info: ModelInfo, runs: int) -> list[Timing]:
    """Time the model on the CPU and on every plug-in device found."""
    timings = [
        time_session(
            ort.InferenceSession(str(path), providers=["CPUExecutionProvider"]),
            info,
            "CPU",
            runs,
        )
    ]
    for device in accelerator_devices():
        label = f"{device.ep_name} {str(device.device.type).split('.')[-1]}"
        try:
            options = ort.SessionOptions()
            options.add_provider_for_devices([device], {})
            session = ort.InferenceSession(str(path), sess_options=options)
            timings.append(time_session(session, info, label, runs))
        except Exception as error:  # noqa: BLE001 - report and carry on with other devices
            timings.append(
                Timing(f"{label} (failed: {str(error)[:120]})", float("nan"), float("nan"), ())
            )
    return timings


def render(info: ModelInfo, timings: list[Timing]) -> str:
    """Format the findings as markdown."""
    lines = [
        f"## {Path(info.path).name}",
        "",
        f"- Size: {info.size_bytes / 1_000_000:.2f} MB",
        f"- SHA-256: `{info.sha256}`",
        f"- Opset: {info.opset}",
    ]
    for kind, tensors in (("Input", info.inputs), ("Output", info.outputs)):
        for t in tensors:
            lines.append(f"- {kind} `{t.name}`: shape {list(t.shape)}, element type {t.elem_type}")
    lines += [
        "",
        "| Provider | Median ms | 95th percentile ms | Providers in use |",
        "| --- | --- | --- | --- |",
    ]
    for timing in timings:
        lines.append(
            f"| {timing.label} | {timing.median_ms:.2f} | {timing.p95_ms:.2f} "
            f"| {', '.join(timing.providers)} |"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", type=Path, help="path to an .onnx file")
    parser.add_argument("--runs", type=int, default=50, help="timed runs per provider")
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("--runs must be at least 1")
    info = describe(args.model)
    print(render(info, benchmark(args.model, info, args.runs)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
