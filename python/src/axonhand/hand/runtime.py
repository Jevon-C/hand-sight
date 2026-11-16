"""Open an ONNX model on the Qualcomm NPU when there is one, otherwise on the CPU.

Which one actually ran is always reported, so a result can say what produced it and a slow run
can be explained.
"""

from __future__ import annotations

from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any, Literal, Protocol

import numpy as np
import onnxruntime as ort
from numpy.typing import NDArray

Device = Literal["auto", "cpu", "npu"]


class ModelError(Exception):
    """A model file could not be loaded or run. The message is for developers."""


class Model(Protocol):
    """Anything that can run a model on one input. Tests use stand-ins for it."""

    def run(self, blob: NDArray[np.float32]) -> list[NDArray[np.float32]]:
        """Run on one input and return the outputs in file order."""
        ...


@dataclass
class LoadedModel:
    """An open model and the device it runs on."""

    session: ort.InferenceSession
    device: str
    """``"NPU"`` or ``"CPU"``: where the model's work is placed."""
    input_name: str
    output_names: tuple[str, ...]
    note: str = ""
    """Why the CPU is in use when the NPU was wanted; empty when nothing fell back."""

    def run(self, blob: NDArray[np.float32]) -> list[NDArray[np.float32]]:
        """Run the model on one input and return its outputs in file order."""
        try:
            outputs = self.session.run(list(self.output_names), {self.input_name: blob})
        except Exception as error:  # noqa: BLE001 - onnxruntime raises several error types
            raise ModelError(f"model run failed: {error}") from error
        return [np.asarray(o, dtype=np.float32) for o in outputs]


@cache
def _register_qualcomm() -> bool:
    """Register the Qualcomm provider once per process (a second registration raises)."""
    try:
        import onnxruntime_qnn as qnn

        ort.register_execution_provider_library(qnn.get_ep_name(), qnn.get_library_path())
    except Exception:  # noqa: BLE001 - any failure here just means "no accelerator"
        return False
    return True


def accelerator_devices() -> list[Any]:
    """Register the Qualcomm provider if its package is installed and list its devices (NPU, GPU).

    Returns an empty list when the package or a device is not available.
    """
    if not _register_qualcomm():
        return []
    return [d for d in ort.get_ep_devices() if d.ep_name != "CPUExecutionProvider"]


def npu_devices() -> list[Any]:
    """The accelerator devices that are NPUs."""
    return [d for d in accelerator_devices() if str(d.device.type).split(".")[-1] == "NPU"]


def open_model(path: Path, device: Device = "auto") -> LoadedModel:
    """Load ``path``.

    ``"auto"`` tries the NPU and falls back to the CPU; ``"npu"`` fails if there is no NPU;
    ``"cpu"`` never uses it.

    Raises:
        ModelError: if the file cannot be loaded, or the NPU was demanded and is not available.
    """
    if not path.is_file():
        raise ModelError(f"model file not found: {path}")
    session: ort.InferenceSession | None = None
    used = "CPU"
    note = ""
    if device != "cpu":
        devices = npu_devices()
        if devices:
            try:
                options = ort.SessionOptions()
                options.add_provider_for_devices([devices[0]], {})
                session = ort.InferenceSession(str(path), sess_options=options)
                used = "NPU"
            except Exception as error:  # noqa: BLE001 - fall back unless the NPU was demanded
                if device == "npu":
                    raise ModelError(f"the NPU could not load {path.name}: {error}") from error
                note = f"the NPU could not load {path.name}: {str(error)[:300]}"
        elif device == "npu":
            raise ModelError("no NPU was found")
        else:
            note = "no NPU was found"
    if session is None:
        try:
            session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
        except Exception as error:  # noqa: BLE001
            raise ModelError(f"cannot load {path.name}: {error}") from error
    return LoadedModel(
        session=session,
        device=used,
        input_name=session.get_inputs()[0].name,
        output_names=tuple(o.name for o in session.get_outputs()),
        note=note,
    )
