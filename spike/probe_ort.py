"""Probe ONNX Runtime on this machine: CPU baseline and Qualcomm NPU (QNN) attempts.

Prints markdown bullet lines. Never raises: every step reports its own error.
"""

from __future__ import annotations

import glob
import os
import platform
import statistics
import sys
import time
import traceback


def log(key: str, value: object) -> None:
    print(f"- {key}: {value}")


def short_err(exc: BaseException) -> str:
    text = f"{type(exc).__name__}: {exc}"
    return " ".join(text.split())[:300]


def build_model(path: str) -> bool:
    try:
        import numpy as np
        import onnx
        from onnx import TensorProto, helper, numpy_helper

        rng = np.random.default_rng(0)
        w1 = numpy_helper.from_array(rng.standard_normal((16, 3, 3, 3)).astype(np.float32), "w1")
        w2 = numpy_helper.from_array(rng.standard_normal((32, 16, 3, 3)).astype(np.float32), "w2")
        nodes = [
            helper.make_node("Conv", ["x", "w1"], ["c1"], strides=[2, 2], pads=[1, 1, 1, 1]),
            helper.make_node("Relu", ["c1"], ["r1"]),
            helper.make_node("Conv", ["r1", "w2"], ["c2"], strides=[2, 2], pads=[1, 1, 1, 1]),
            helper.make_node("Relu", ["c2"], ["r2"]),
            helper.make_node("GlobalAveragePool", ["r2"], ["y"]),
        ]
        graph = helper.make_graph(
            nodes,
            "probe",
            [helper.make_tensor_value_info("x", TensorProto.FLOAT, [1, 3, 224, 224])],
            [helper.make_tensor_value_info("y", TensorProto.FLOAT, [1, 32, 1, 1])],
            [w1, w2],
        )
        model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
        model.ir_version = 8
        onnx.checker.check_model(model)
        onnx.save(model, path)
        return True
    except Exception as exc:  # noqa: BLE001
        log("build tiny model", f"FAIL {short_err(exc)}")
        return False


def time_session(sess: object, label: str) -> None:
    import numpy as np

    x = np.random.default_rng(1).standard_normal((1, 3, 224, 224)).astype(np.float32)
    try:
        for _ in range(5):
            sess.run(None, {"x": x})  # type: ignore[attr-defined]
        times = []
        for _ in range(30):
            t0 = time.perf_counter()
            sess.run(None, {"x": x})  # type: ignore[attr-defined]
            times.append((time.perf_counter() - t0) * 1000)
        log(f"{label} median ms (30 runs)", f"{statistics.median(times):.2f}")
        log(f"{label} providers in use", sess.get_providers())  # type: ignore[attr-defined]
    except Exception as exc:  # noqa: BLE001
        log(f"{label} run", f"FAIL {short_err(exc)}")


def main() -> None:
    log("python", f"{platform.machine()} {sys.version.split()[0]}")
    try:
        import onnxruntime as ort
    except Exception as exc:  # noqa: BLE001
        log("import onnxruntime", f"FAIL {short_err(exc)}")
        return
    log("onnxruntime version", ort.__version__)
    log("available providers", ort.get_available_providers())

    model_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_probe_model.onnx")
    if not build_model(model_path):
        return

    try:
        sess = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])
        time_session(sess, "CPU")
    except Exception as exc:  # noqa: BLE001
        log("CPU session", f"FAIL {short_err(exc)}")

    # Qualcomm package: describe what it ships.
    qnn_mod = None
    try:
        import onnxruntime_qnn as qnn_mod  # type: ignore[import-not-found,no-redef]

        log("import onnxruntime_qnn", f"OK, attributes: {[a for a in dir(qnn_mod) if not a.startswith('_')]}")
    except Exception as exc:  # noqa: BLE001
        log("import onnxruntime_qnn", f"FAIL {short_err(exc)}")

    site = os.path.dirname(os.path.dirname(ort.__file__))
    dlls = glob.glob(os.path.join(site, "**", "*[Qq]nn*.dll"), recursive=True)
    log("QNN-related DLLs found", [os.path.relpath(d, site) for d in dlls][:12] or "none")

    # Newer ORT: plugin execution providers are registered from a library path.
    if hasattr(ort, "get_ep_devices"):
        try:
            log("ep devices before registration", [(d.ep_name, str(d.device.type)) for d in ort.get_ep_devices()])
        except Exception as exc:  # noqa: BLE001
            log("get_ep_devices", f"FAIL {short_err(exc)}")
    lib = None
    if qnn_mod is not None and hasattr(qnn_mod, "get_library_path"):
        try:
            lib = qnn_mod.get_library_path()
            log("qnn get_library_path()", lib)
        except Exception as exc:  # noqa: BLE001
            log("qnn get_library_path()", f"FAIL {short_err(exc)}")
    if lib is None:
        ep_libs = [d for d in dlls if "execution_provider" in os.path.basename(d).lower()]
        lib = ep_libs[0] if ep_libs else None
    if lib and hasattr(ort, "register_execution_provider_library"):
        try:
            ort.register_execution_provider_library("QNNExecutionProvider", lib)
            log("register_execution_provider_library", "OK")
            if hasattr(ort, "get_ep_devices"):
                log("ep devices after registration", [(d.ep_name, str(d.device.type)) for d in ort.get_ep_devices()])
        except Exception as exc:  # noqa: BLE001
            log("register_execution_provider_library", f"FAIL {short_err(exc)}")

    attempts = [
        ("QNN HTP (NPU)", [("QNNExecutionProvider", {"backend_type": "htp"}), "CPUExecutionProvider"]),
        ("QNN HTP by dll", [("QNNExecutionProvider", {"backend_path": "QnnHtp.dll"}), "CPUExecutionProvider"]),
        ("QNN GPU", [("QNNExecutionProvider", {"backend_type": "gpu"}), "CPUExecutionProvider"]),
    ]
    for label, providers in attempts:
        try:
            sess = ort.InferenceSession(model_path, providers=providers)
            used = sess.get_providers()
            log(f"{label} session", f"created, providers {used}")
            if "QNNExecutionProvider" in used:
                time_session(sess, label)
        except Exception as exc:  # noqa: BLE001
            log(f"{label} session", f"FAIL {short_err(exc)}")

    try:
        os.remove(model_path)
    except OSError:
        traceback.print_exc()


if __name__ == "__main__":
    main()
