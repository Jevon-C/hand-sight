"""Measure the software against tap times marked by a person.

Usage::

    python -m axonhand.evaluate FOLDER [--models DIR] [--device auto|cpu|npu] [--out DIR]

Every video in ``FOLDER`` that has a ``<video>.taps.json`` beside it (made with
``python -m axonhand.tools.annotate``) is tracked with the real hand model and analysed, and the
result is compared with the marked taps. Writes ``report.md`` and ``report.json`` to ``--out``
(default ``data/eval``) and prints the summary. Exit status 0 on success, 1 if there is nothing to
evaluate or the models are unavailable.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path
from typing import cast

from axonhand.evaluation import tapmarks
from axonhand.evaluation.report import markdown, summarise, to_json, verdicts
from axonhand.evaluation.scoring import ClipScore, Failure, score_clip
from axonhand.hand.runtime import Device, ModelError, open_model
from axonhand.hand.video import TrackingError, track_video
from axonhand.hand_model import LANDMARK_MODEL, PALM_MODEL, PIPELINE_VERSION

VIDEO_SUFFIXES = {".mp4", ".mov", ".m4v", ".avi", ".mkv", ".webm"}


def find_clips(folder: Path) -> list[Path]:
    """Videos in ``folder`` that have an annotation, in name order."""
    return sorted(
        p
        for p in folder.iterdir()
        if p.suffix.lower() in VIDEO_SUFFIXES and tapmarks.annotation_path(p).is_file()
    )


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    parser.add_argument(
        "--models", type=Path, default=Path(os.environ.get("AXONHAND_MODELS", "models"))
    )
    parser.add_argument("--device", choices=["auto", "cpu", "npu"], default="auto")
    parser.add_argument("--out", type=Path, default=Path("data/eval"))
    args = parser.parse_args(argv)
    clips = find_clips(args.folder)
    if not clips:
        print(f"no videos with a .taps.json file in {args.folder}", file=sys.stderr)
        return 1
    try:
        device = cast(Device, args.device)
        palm = open_model(args.models / PALM_MODEL, device)
        landmarks = open_model(args.models / LANDMARK_MODEL, device)
    except ModelError as error:
        print(f"hand model unavailable: {error}", file=sys.stderr)
        return 1
    scores: list[ClipScore] = []
    failures: list[Failure] = []
    for clip in clips:
        annotation = tapmarks.load(clip)
        if annotation is None or len(annotation.times) < 4:
            print(f"skipped {clip.name}: fewer than 4 taps marked")
            continue
        try:
            track, _ = track_video(clip, palm, landmarks)
        except TrackingError as error:
            failures.append(Failure(clip.name, str(error)))
            print(f"FAILED  {clip.name}: {error}")
            continue
        outcome = score_clip(clip.name, track, annotation.times)
        if isinstance(outcome, Failure):
            failures.append(outcome)
            print(f"FAILED  {clip.name}: {outcome.reason}")
        else:
            scores.append(outcome)
            print(
                f"ok      {clip.name}: marked {outcome.marked_count}, found "
                f"{outcome.found_count}, rate error {outcome.rate_error_percent:+.1f}%"
            )
            for label, times in (
                ("marked, not found at", outcome.missed_times),
                ("found, not marked at", outcome.extra_times),
            ):
                if times:
                    print(f"        {label}: " + ", ".join(f"{t:.2f} s" for t in times))
    pipeline = f"{PIPELINE_VERSION} on {palm.device}/{landmarks.device}"
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "report.md").write_text(markdown(scores, failures, pipeline), encoding="utf-8")
    (args.out / "report.json").write_text(to_json(scores, failures, pipeline), encoding="utf-8")
    summary = summarise(scores, failures)
    print()
    print(f"clips {summary.clips}, failed {summary.failures}, marked taps {summary.marked_taps}")
    print(f"recall {summary.recall:.1%}, precision {summary.precision:.1%}")
    print(
        f"rate error mean {summary.rate_error_mean_abs_percent:.2f}%, "
        f"worst {summary.rate_error_worst_percent:.2f}%"
    )
    print(
        f"tap time: bias {summary.timing_bias_ms:+.1f} ms, "
        f"95% within {summary.timing_error_p95_abs_ms:.1f} ms"
    )
    for name, ok in verdicts(summary):
        print(f"{'MET    ' if ok else 'NOT MET'} {name}")
    print(f"report written to {args.out / 'report.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
