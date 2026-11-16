"""Run a small convnet on CPU and on each plug-in EP device (NPU/GPU) of the Qualcomm package.

Uses the device-based API: register the library, then add_provider_for_devices.
Prints markdown bullet lines; never raises.
"""

from __future__ import annotations

import os
import platform
import statistics
import sys
import tempfile
import time

import numpy as np


def log(key: str, value: object) -> None:
    print(f"- {key}: {value}")


def err(exc: BaseException) -> str:
    return " ".join(f"{type(exc).__name__}: {exc}".split())[:300]


def build(path: str) -> None:
    import onnx
    from onnx import TensorProto, helper, numpy_helper

    rng = np.random.default_rng(0)
    chans = [3, 64, 128, 256, 256]
    nodes, inits, prev = [], [], "x"
    for i in range(4):
        w = numpy_helper.from_array(
            (rng.standard_normal((chans[i + 1], chans[i], 3, 3)) * 0.1).astype(
                np.float32
            ),
            f"w{i}",
        )
        inits.append(w)
        nodes.append(
            helper.make_node(
                "Conv", [prev, f"w{i}"], [f"c{i}"], strides=[2, 2], pads=[1, 1, 1, 1]
            )
        )
        nodes.append(helper.make_node("Relu", [f"c{i}"], [f"r{i}"]))
        prev = f"r{i}"
    nodes.append(helper.make_node("GlobalAveragePool", [prev], ["y"]))
    graph = helper.make_graph(
        nodes,
        "probe",
        [helper.make_tensor_value_info("x", TensorProto.FLOAT, [1, 3, 384, 384])],
        [helper.make_tensor_value_info("y", TensorProto.FLOAT, [1, 256, 1, 1])],
        inits,
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 8
    onnx.save(model, path)


def quantise(src: str, dst: str) -> None:
    from onnxruntime.quantization import (
        CalibrationDataReader,
        QuantFormat,
        QuantType,
        quantize_static,
    )

    class Reader(CalibrationDataReader):
        def __init__(self) -> None:
            rng = np.random.default_rng(2)
            self.items = iter(
                [
                    {"x": rng.standard_normal((1, 3, 384, 384)).astype(np.float32)}
                    for _ in range(8)
                ]
            )

        def get_next(self):  # type: ignore[no-untyped-def]
            return next(self.items, None)

    quantize_static(
        src,
        dst,
        Reader(),
        quant_format=QuantFormat.QDQ,
        activation_type=QuantType.QUInt8,
        weight_type=QuantType.QUInt8,
    )


def bench(sess: object, label: str, ref: np.ndarray | None) -> np.ndarray | None:
    x = np.random.default_rng(1).standard_normal((1, 3, 384, 384)).astype(np.float32)
    try:
        out = None
        for _ in range(5):
            out = sess.run(None, {"x": x})[0]  # type: ignore[attr-defined]
        times = []
        for _ in range(40):
            t0 = time.perf_counter()
            sess.run(None, {"x": x})  # type: ignore[attr-defined]
            times.append((time.perf_counter() - t0) * 1000)
        diff = (
            ""
            if ref is None or out is None
            else f", max abs diff vs CPU fp32 {float(np.abs(out - ref).max()):.4f}"
        )
        log(label, f"median {statistics.median(times):.2f} ms{diff}")
        return out
    except Exception as exc:  # noqa: BLE001
        log(label, f"FAIL {err(exc)}")
        return None


def provider_counts(path: str, device: object) -> None:
    import json

    import onnxruntime as ort

    try:
        so = ort.SessionOptions()
        so.enable_profiling = True
        so.add_provider_for_devices([device], {})
        sess = ort.InferenceSession(path, sess_options=so)
        x = (
            np.random.default_rng(3)
            .standard_normal((1, 3, 384, 384))
            .astype(np.float32)
        )
        for _ in range(3):
            sess.run(None, {"x": x})
        prof = sess.end_profiling()
        with open(prof, encoding="utf-8") as fh:
            events = json.load(fh)
        counts: dict[str, int] = {}
        for ev in events:
            if ev.get("cat") == "Node" and ev.get("name", "").endswith("_kernel_time"):
                prov = ev.get("args", {}).get("provider", "unknown")
                counts[prov] = counts.get(prov, 0) + 1
        log("node executions by provider (profile)", counts or "none recorded")
    except Exception as exc:  # noqa: BLE001
        log("profile", f"FAIL {err(exc)}")


def main() -> None:
    log("python", f"{platform.machine()} {sys.version.split()[0]}")
    try:
        import onnxruntime as ort
    except Exception as exc:  # noqa: BLE001
        log("import onnxruntime", f"FAIL {err(exc)}")
        return
    tmp = tempfile.mkdtemp()
    fp32, q8 = os.path.join(tmp, "m32.onnx"), os.path.join(tmp, "m8.onnx")
    try:
        build(fp32)
    except Exception as exc:  # noqa: BLE001
        log("build model", f"FAIL {err(exc)}")
        return
    has_q = True
    try:
        quantise(fp32, q8)
    except Exception as exc:  # noqa: BLE001
        has_q = False
        log("quantise int8 (QDQ)", f"FAIL {err(exc)}")

    ref = bench(
        ort.InferenceSession(fp32, providers=["CPUExecutionProvider"]), "CPU fp32", None
    )
    if has_q:
        bench(
            ort.InferenceSession(q8, providers=["CPUExecutionProvider"]),
            "CPU int8",
            ref,
        )

    try:
        import onnxruntime_qnn as qnn

        ort.register_execution_provider_library(
            qnn.get_ep_name(), qnn.get_library_path()
        )
        log("registered EP", qnn.get_ep_name())
    except Exception as exc:  # noqa: BLE001
        log("register QNN", f"FAIL {err(exc)}")

    devices = [d for d in ort.get_ep_devices() if d.ep_name != "CPUExecutionProvider"]
    log("plug-in devices", [(d.ep_name, str(d.device.type)) for d in devices] or "none")
    for d in devices:
        kind = str(d.device.type).split(".")[-1]
        for label, path in (("fp32", fp32), ("int8", q8 if has_q else None)):
            if path is None:
                continue
            name = f"{d.ep_name} {kind} {label}"
            try:
                so = ort.SessionOptions()
                so.add_provider_for_devices([d], {})
                sess = ort.InferenceSession(path, sess_options=so)
                log(f"{name} providers in use", sess.get_providers())
                bench(sess, name, ref)
                provider_counts(path, d)
            except Exception as exc:  # noqa: BLE001
                log(name, f"FAIL {err(exc)}")


if __name__ == "__main__":
    main()
