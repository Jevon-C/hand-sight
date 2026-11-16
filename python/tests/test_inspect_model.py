"""Tests for the model inspection tool, using a tiny model built on the fly."""

from pathlib import Path

import numpy as np
import onnx
import pytest
from onnx import TensorProto, helper, numpy_helper

from axonhand.tools import inspect_model


def tiny_model(path: Path) -> None:
    weight = numpy_helper.from_array(np.ones((4, 3, 3, 3), dtype=np.float32), "w")
    graph = helper.make_graph(
        [helper.make_node("Conv", ["x", "w"], ["y"], pads=[1, 1, 1, 1])],
        "tiny",
        [helper.make_tensor_value_info("x", TensorProto.FLOAT, ["batch", 3, 16, 16])],
        [helper.make_tensor_value_info("y", TensorProto.FLOAT, ["batch", 4, 16, 16])],
        [weight],
    )
    model = helper.make_model(graph, opset_imports=[helper.make_opsetid("", 13)])
    model.ir_version = 8
    onnx.save(model, str(path))


def test_describe_reads_shapes_hash_and_opset(tmp_path: Path) -> None:
    path = tmp_path / "tiny.onnx"
    tiny_model(path)
    info = inspect_model.describe(path)
    assert info.opset == 13
    assert [t.name for t in info.inputs] == ["x"]  # the weight is not an input
    assert info.inputs[0].shape == ("batch", 3, 16, 16)
    assert info.inputs[0].concrete_shape == (1, 3, 16, 16)
    assert len(info.sha256) == 64
    assert info.size_bytes == path.stat().st_size


def test_benchmark_times_the_cpu(tmp_path: Path) -> None:
    path = tmp_path / "tiny.onnx"
    tiny_model(path)
    info = inspect_model.describe(path)
    timings = inspect_model.benchmark(path, info, runs=5)
    assert timings[0].label == "CPU"
    assert timings[0].median_ms > 0.0
    assert "CPUExecutionProvider" in timings[0].providers


def test_render_is_markdown(tmp_path: Path) -> None:
    path = tmp_path / "tiny.onnx"
    tiny_model(path)
    info = inspect_model.describe(path)
    text = inspect_model.render(info, inspect_model.benchmark(path, info, runs=3))
    assert text.startswith("## tiny.onnx")
    assert "| CPU |" in text


def test_missing_file_is_reported(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        inspect_model.describe(tmp_path / "absent.onnx")


def test_main_rejects_zero_runs(tmp_path: Path) -> None:
    with pytest.raises(SystemExit):
        inspect_model.main([str(tmp_path / "x.onnx"), "--runs", "0"])
