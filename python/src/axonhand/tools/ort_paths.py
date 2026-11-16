"""Tell the Rust tracker where ONNX Runtime and the NPU plug-in are.

Usage: ``python -m axonhand.tools.ort_paths``

Prints one JSON object: ``library`` (the ONNX Runtime shared library inside the installed
``onnxruntime`` package), and ``npu_name`` and ``npu_library`` (the Qualcomm provider, or null
when ``onnxruntime-qnn`` is not installed). The Rust program loads the same runtime the Python
pipeline uses, so both run the same version of the same engine.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def find_library() -> Path | None:
    """The ONNX Runtime shared library of the installed ``onnxruntime`` package."""
    import onnxruntime

    folder = Path(onnxruntime.__file__).parent / "capi"
    for pattern in ("onnxruntime.dll", "libonnxruntime.so*", "libonnxruntime*.dylib"):
        found = sorted(p for p in folder.glob(pattern) if p.is_file())
        if found:
            return found[0]
    return None


def qnn_provider() -> tuple[str, Path] | None:
    """Name and library of the Qualcomm provider, or ``None`` if it is not installed."""
    try:
        import onnxruntime_qnn as qnn

        return str(qnn.get_ep_name()), Path(str(qnn.get_library_path()))
    except Exception:  # noqa: BLE001 - any failure here just means "no NPU provider"
        return None


def describe() -> dict[str, str | None]:
    """The paths as the plain data the tool prints."""
    library = find_library()
    npu = qnn_provider()
    return {
        "library": str(library) if library else None,
        "npu_name": npu[0] if npu else None,
        "npu_library": str(npu[1]) if npu else None,
    }


def main() -> int:
    """Command line entry point."""
    try:
        print(json.dumps(describe()))
    except ImportError:
        print("onnxruntime is not installed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
