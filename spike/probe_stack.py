"""Check torch, mediapipe, mujoco and PyAV actually import and do a tiny task.

Prints markdown bullet lines; never raises.
"""

from __future__ import annotations

import os
import platform
import sys
import tempfile


def log(key: str, value: object) -> None:
    print(f"- {key}: {value}")


def err(exc: BaseException) -> str:
    return " ".join(f"{type(exc).__name__}: {exc}".split())[:300]


def check_torch() -> None:
    try:
        import torch

        log("torch import", f"OK {torch.__version__}")
        net = torch.nn.Sequential(torch.nn.Conv2d(3, 8, 3), torch.nn.ReLU())
        x = torch.randn(1, 3, 32, 32)
        net(x).sum().backward()
        log("torch forward and backward", "OK")
        path = os.path.join(tempfile.mkdtemp(), "t.onnx")
        torch.onnx.export(net, (x,), path, input_names=["x"], dynamo=False)
        import onnxruntime as ort

        out = ort.InferenceSession(path, providers=["CPUExecutionProvider"]).run(
            None, {"x": x.numpy()}
        )[0]
        diff = float(abs(out - net(x).detach().numpy()).max())
        log("torch to ONNX export and ORT check", f"OK, max abs diff {diff:.6f}")
    except Exception as exc:  # noqa: BLE001
        log("torch", f"FAIL {err(exc)}")


def check_mediapipe() -> None:
    try:
        import mediapipe as mp
        from mediapipe.tasks.python import vision

        log(
            "mediapipe import",
            f"OK {mp.__version__}, HandLandmarker class: {hasattr(vision, 'HandLandmarker')}",
        )
    except Exception as exc:  # noqa: BLE001
        log("mediapipe", f"FAIL {err(exc)}")


def check_mujoco() -> None:
    try:
        import mujoco

        model = mujoco.MjModel.from_xml_string(
            "<mujoco><worldbody><geom size='1'/></worldbody></mujoco>"
        )
        data = mujoco.MjData(model)
        mujoco.mj_step(model, data)
        log("mujoco import and step", f"OK {mujoco.__version__}")
    except Exception as exc:  # noqa: BLE001
        log("mujoco", f"FAIL {err(exc)}")


def check_av() -> None:
    try:
        import av
        import numpy as np

        path = os.path.join(tempfile.mkdtemp(), "t.mp4")
        used = None
        for codec in ("libx264", "mpeg4"):
            try:
                with av.open(path, "w") as out:
                    stream = out.add_stream(codec, rate=30)
                    stream.width, stream.height, stream.pix_fmt = 64, 48, "yuv420p"
                    for i in range(10):
                        frame = av.VideoFrame.from_ndarray(
                            np.full((48, 64, 3), i * 20, np.uint8), format="rgb24"
                        )
                        for pkt in stream.encode(frame):
                            out.mux(pkt)
                    for pkt in stream.encode():
                        out.mux(pkt)
                used = codec
                break
            except Exception as exc:  # noqa: BLE001
                log(f"PyAV codec {codec}", f"unavailable: {err(exc)}")
        if used is None:
            log("PyAV encode", "FAIL no codec worked")
            return
        with av.open(path) as inp:
            frames = [f.to_ndarray(format="rgb24") for f in inp.decode(video=0)]
        log(
            "PyAV encode then decode",
            f"OK codec {used}, {len(frames)} frames, shape {frames[0].shape}",
        )
    except Exception as exc:  # noqa: BLE001
        log("PyAV", f"FAIL {err(exc)}")


def main() -> None:
    log("python", f"{platform.machine()} {sys.version.split()[0]}")
    check_av()
    check_torch()
    check_mediapipe()
    check_mujoco()


if __name__ == "__main__":
    main()
