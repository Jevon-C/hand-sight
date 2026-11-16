"""The parity fixture tool writes what the Rust tests expect."""

from __future__ import annotations

from pathlib import Path

import pytest

from axonhand.tools import parity_fixture
from axonhand.tools.parity_fixture import build, main, model_input


def test_build_has_every_section_without_models() -> None:
    data = build(None)
    assert set(data) == {"anchors", "geometry", "decode", "image"}
    assert data["anchors"]["count"] == 2016
    assert len(data["decode"]["palms"]) >= 1


def test_build_is_repeatable() -> None:
    assert build(None) == build(None)


def test_model_input_is_a_formula_the_rust_side_can_repeat() -> None:
    blob = model_input(4)
    assert blob.shape == (1, 4, 4, 3)
    assert float(blob.reshape(-1)[1]) == pytest.approx(37 / 255)
    assert float(blob.reshape(-1)[7]) == pytest.approx((7 * 37 % 255) / 255)


def test_main_writes_the_file(tmp_path: Path) -> None:
    target = tmp_path / "nested" / "parity.json"
    assert main([str(target)]) == 0
    assert target.stat().st_size > 1000


def test_clip_export_needs_models(tmp_path: Path) -> None:
    with pytest.raises(SystemExit):
        main([str(tmp_path), "--clip", str(tmp_path / "x.mp4")])


def test_module_has_a_command_line() -> None:
    assert callable(parity_fixture.main)
