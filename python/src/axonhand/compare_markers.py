"""Compare two people's tap marks on the same clips.

Usage::

    python -m axonhand.compare_markers FIRST_FOLDER SECOND_FOLDER [--out DIR]

Each folder holds copies of the same videos with a ``<video>.taps.json`` made by one person.
Clips marked in both folders are compared; writes ``agreement.md`` to ``--out`` (default
``data/eval``) and prints the summary. Exit status 0 on success, 1 if no clip was marked in both.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from axonhand.evaluate import VIDEO_SUFFIXES
from axonhand.evaluation import tapmarks
from axonhand.evaluation.agreement import Agreement, compare_marks, markdown, summarise
from axonhand.evaluation.tapmarks import AnnotationError
from axonhand.tools.probe import ProbeError, probe


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    parser.add_argument("--out", type=Path, default=Path("data/eval"))
    args = parser.parse_args(argv)
    items: list[Agreement] = []
    for video in sorted(p for p in args.first.iterdir() if p.suffix.lower() in VIDEO_SUFFIXES):
        other = args.second / video.name
        try:
            first = tapmarks.load(video)
            second = tapmarks.load(other)
        except AnnotationError as error:
            print(f"skipped {video.name}: {error}")
            continue
        if first is None or second is None or len(first.times) < 4:
            continue
        try:
            fps = probe(video).fps
        except ProbeError as error:
            print(f"skipped {video.name}: {error}")
            continue
        item = compare_marks(video.name, first.times, second.times, fps)
        items.append(item)
        print(
            f"{video.name}: {item.first_count} vs {item.second_count} marks, {item.hits} in common"
        )
    if not items:
        print("no clip is marked in both folders", file=sys.stderr)
        return 1
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "agreement.md").write_text(markdown(items), encoding="utf-8")
    s = summarise(items)
    print()
    print(f"clips {s.clips}; both marked {s.agreement:.1%} of all marks")
    print(
        f"time difference: bias {s.bias_ms:+.1f} ms, median {s.median_abs_ms:.1f} ms, "
        f"95% within {s.p95_abs_ms:.1f} ms"
    )
    print(f"report written to {args.out / 'agreement.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
