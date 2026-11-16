"""Check whether mediapipe works on this machine when installed without opencv-contrib."""

from __future__ import annotations

import platform
import sys


def log(key: str, value: object) -> None:
    print(f"- {key}: {value}")


def err(exc: BaseException) -> str:
    return " ".join(f"{type(exc).__name__}: {exc}".split())[:300]


def main() -> None:
    log("python", f"{platform.machine()} {sys.version.split()[0]}")
    try:
        import mediapipe as mp

        log("import mediapipe", f"OK {getattr(mp, '__version__', '?')}")
    except Exception as exc:  # noqa: BLE001
        log("import mediapipe", f"FAIL {err(exc)}")
        return
    try:
        from mediapipe.tasks.python import vision

        log("tasks.vision", f"OK, HandLandmarker: {hasattr(vision, 'HandLandmarker')}")
    except Exception as exc:  # noqa: BLE001
        log("tasks.vision", f"FAIL {err(exc)}")
    try:
        import numpy as np

        img = mp.Image(
            image_format=mp.ImageFormat.SRGB, data=np.zeros((64, 64, 3), np.uint8)
        )
        log("mp.Image from numpy", f"OK {img.width}x{img.height}")
    except Exception as exc:  # noqa: BLE001
        log("mp.Image from numpy", f"FAIL {err(exc)}")
    log("cv2 loaded as a side effect", "cv2" in sys.modules)


if __name__ == "__main__":
    main()
