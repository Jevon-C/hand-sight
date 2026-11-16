"""Compare two sets of landmarks for the same frames, in palm lengths.

Used to compare the Rust tracker (on the NPU or the CPU) with the Python pipeline: both write a
JSON list with one entry per frame, ``null`` for no hand or 21 rows of x, y, depth. The error of a
landmark is its distance from the reference point divided by the reference hand's palm length
(wrist to middle-finger base), so it does not depend on the picture size.

Run: ``python -m axonhand.tools.compare_landmarks REFERENCE OTHER [--fps 30]``, where each is a
JSON file as above or the ``.npz`` track that ``axonhand.hand_model`` and ``axonhand-track`` write.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from numpy.typing import NDArray

from axonhand.landmarks import LandmarkTrack
from axonhand.tapping import AnalysisError
from axonhand.tools.quantize import MAX_MEDIAN_ERROR, MAX_P95_ERROR, compare_tracks

_WRIST, _MIDDLE_MCP = 0, 9


@dataclass(frozen=True)
class Difference:
    """How far one set of landmarks is from another."""

    frames: int
    both_found: int
    disagree: int
    """Frames where exactly one of the two found a hand."""
    median: float
    p95: float
    maximum: float
    worst_frame: int
    worst_landmark: int
    """Landmark number (0 is the wrist) of the largest error."""
    over_limit: int = 0
    """In how many frames some landmark is further off than the 95th percentile limit ."""
    first_difference: int | None = None
    """The first frame where some landmark differs by more than 1e-4 palm lengths, if any."""
    first_errors: tuple[float, ...] = ()
    """The largest landmark error in each of the first ten frames both have a hand in."""
    worst_frames: tuple[tuple[int, float], ...] = ()
    """The frames with the largest error (frame number, largest landmark error), worst first."""


_VISIBLE = 1e-4
"""An error below this (in palm lengths) is float noise, far below what any measure can see."""


def load(path: Path) -> list[NDArray[np.float64] | None]:
    """Read a landmark file: JSON, or the ``.npz`` track the trackers write.

    In a ``.npz`` track a frame whose landmarks are all NaN is a frame with no hand.
    """
    if path.suffix == ".npz":
        track = LandmarkTrack.load(path)
        return [None if np.isnan(frame).all() else frame for frame in track.points]
    raw = json.loads(path.read_text(encoding="utf-8"))
    return [None if item is None else np.asarray(item, dtype=np.float64) for item in raw]


def compare(
    reference: list[NDArray[np.float64] | None], other: list[NDArray[np.float64] | None]
) -> Difference:
    """The error of ``other`` against ``reference`` over the frames both have.

    Raises:
        ValueError: if the two cover different numbers of frames or no frame has a hand in both.
    """
    if len(reference) != len(other):
        raise ValueError("the two files cover different numbers of frames")
    errors: list[NDArray[np.float64]] = []
    frame_of: list[int] = []
    disagree = 0
    for index, (a, b) in enumerate(zip(reference, other, strict=True)):
        if (a is None) != (b is None):
            disagree += 1
        if a is None or b is None:
            continue
        palm = float(np.linalg.norm(a[_WRIST, :2] - a[_MIDDLE_MCP, :2]))
        errors.append(np.linalg.norm(a[:, :2] - b[:, :2], axis=1) / max(palm, 1e-6))
        frame_of.append(index)
    if not errors:
        raise ValueError("no frame has a hand in both files")
    stacked = np.stack(errors)
    per_frame = stacked.max(axis=1)
    worst_row, worst_landmark = np.unravel_index(int(np.argmax(stacked)), stacked.shape)
    return Difference(
        frames=len(reference),
        both_found=len(errors),
        disagree=disagree,
        median=float(np.median(stacked)),
        p95=float(np.percentile(stacked, 95)),
        maximum=float(stacked.max()),
        worst_frame=frame_of[int(worst_row)],
        worst_landmark=int(worst_landmark),
        first_difference=next(
            (frame_of[i] for i, error in enumerate(per_frame) if error > _VISIBLE), None
        ),
        first_errors=tuple(float(e) for e in per_frame[:10]),
        over_limit=int((stacked.max(axis=1) > MAX_P95_ERROR).sum()),
        worst_frames=tuple(
            (frame_of[int(i)], float(per_frame[int(i)])) for i in np.argsort(per_frame)[::-1][:5]
        ),
    )


def _track(frames: list[NDArray[np.float64] | None], fps: float) -> LandmarkTrack:
    blank = np.full((21, 3), np.nan)
    return LandmarkTrack(fps=fps, points=np.stack([blank if f is None else f for f in frames]))


def main(argv: list[str] | None = None) -> int:
    """Command line entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path)
    parser.add_argument("other", type=Path)
    parser.add_argument(
        "--fps",
        type=float,
        default=None,
        help="frame rate, for the tap analysis (default: the reference's, or 30 for JSON files)",
    )
    args = parser.parse_args(argv)
    reference, other = load(args.reference), load(args.other)
    if args.fps is None:
        args.fps = (
            LandmarkTrack.load(args.reference).fps if args.reference.suffix == ".npz" else 30.0
        )
    try:
        d = compare(reference, other)
    except ValueError as error:
        print(f"error: {error}")  # noqa: T201
        return 1
    print(f"{d.frames} frames; hand found in both for {d.both_found}; disagree on {d.disagree}")  # noqa: T201
    print(  # noqa: T201
        f"error in palm lengths: median {d.median:.4f}, 95th percentile {d.p95:.4f}, "
        f"largest {d.maximum:.4f} (frame {d.worst_frame}, landmark {d.worst_landmark})"
    )
    print(  # noqa: T201
        f"proposed limits: median <= {MAX_MEDIAN_ERROR}, 95th percentile <= {MAX_P95_ERROR}: "
        f"{'met' if d.median <= MAX_MEDIAN_ERROR and d.p95 <= MAX_P95_ERROR else 'NOT met'}"
    )
    print(  # noqa: T201
        "identical in every frame"
        if d.first_difference is None
        else f"first frame that differs by more than {_VISIBLE}: {d.first_difference}"
    )
    print("largest error in the first frames: " + ", ".join(f"{e:.4f}" for e in d.first_errors))  # noqa: T201
    print(  # noqa: T201
        f"frames with some landmark off by more than {MAX_P95_ERROR}: {d.over_limit} of "
        f"{d.both_found}; worst frames: "
        + ", ".join(f"{frame} ({error:.2f})" for frame, error in d.worst_frames)
    )
    try:
        taps = compare_tracks(_track(reference, args.fps), _track(other, args.fps))
    except AnalysisError as error:
        print(f"tap analysis not possible on these frames: {error}")  # noqa: T201
    else:
        print(  # noqa: T201
            f"taps: {taps.float_taps} against {taps.quantised_taps}; contact times differ by at "
            f"most {taps.contact_diff_ms_max:.1f} ms; "
            f"rate differs by {taps.rate_diff_percent:+.2f}%"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
