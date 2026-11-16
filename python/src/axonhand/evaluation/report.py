"""Summaries and the written report of an evaluation."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass

import numpy as np

from axonhand.evaluation.scoring import ClipScore, Failure

MAX_RATE_ERROR_PERCENT = 5.0
MIN_RECALL = 0.95
MIN_PRECISION = 0.95
MAX_TIMING_ERROR_MS = 70.0
MIN_CLIPS = 10
"""Proposed acceptance limits (docs/EVALUATION.md). They are a proposal, to be confirmed."""


@dataclass(frozen=True)
class Summary:
    """Error over a set of clips."""

    clips: int
    failures: int
    marked_taps: int
    recall: float
    precision: float
    count_error_mean_abs: float
    rate_error_mean_abs_percent: float
    rate_error_worst_percent: float
    timing_bias_ms: float
    timing_error_median_abs_ms: float
    timing_error_p95_abs_ms: float
    hesitation_agree: int
    arrest_agree: int


def summarise(scores: list[ClipScore], failures: list[Failure]) -> Summary:
    """Pool the clips. A clip the software could not analyse adds its marked taps as missed."""
    errors = np.array([e for s in scores for e in s.timing_errors], dtype=np.float64) * 1000.0
    hits = sum(s.hits for s in scores)
    marked = sum(s.marked_count for s in scores)
    found = sum(s.found_count for s in scores)
    rate = np.array([abs(s.rate_error_percent) for s in scores], dtype=np.float64)
    count = np.array([abs(s.count_error) for s in scores], dtype=np.float64)
    return Summary(
        clips=len(scores) + len(failures),
        failures=len(failures),
        marked_taps=marked,
        recall=hits / marked if marked else float("nan"),
        precision=hits / found if found else float("nan"),
        count_error_mean_abs=float(count.mean()) if count.size else float("nan"),
        rate_error_mean_abs_percent=float(rate.mean()) if rate.size else float("nan"),
        rate_error_worst_percent=float(rate.max()) if rate.size else float("nan"),
        timing_bias_ms=float(errors.mean()) if errors.size else float("nan"),
        timing_error_median_abs_ms=float(np.median(np.abs(errors)))
        if errors.size
        else float("nan"),
        timing_error_p95_abs_ms=float(np.percentile(np.abs(errors), 95))
        if errors.size
        else float("nan"),
        hesitation_agree=sum(s.found_hesitations == s.marked_hesitations for s in scores),
        arrest_agree=sum(s.found_arrests == s.marked_arrests for s in scores),
    )


def verdicts(summary: Summary) -> list[tuple[str, bool]]:
    """Each proposed limit and whether the summary meets it."""
    return [
        (
            f"at least {MIN_CLIPS} clips, none failed",
            summary.clips >= MIN_CLIPS and not summary.failures,
        ),
        (f"recall at least {MIN_RECALL:.0%}", summary.recall >= MIN_RECALL),
        (f"precision at least {MIN_PRECISION:.0%}", summary.precision >= MIN_PRECISION),
        (
            f"every clip's rate within {MAX_RATE_ERROR_PERCENT:g}%",
            summary.rate_error_worst_percent <= MAX_RATE_ERROR_PERCENT,
        ),
        (
            f"95% of tap times within {MAX_TIMING_ERROR_MS:g} ms",
            summary.timing_error_p95_abs_ms <= MAX_TIMING_ERROR_MS,
        ),
    ]


def markdown(scores: list[ClipScore], failures: list[Failure], pipeline: str) -> str:
    """The report as Markdown."""
    s = summarise(scores, failures)
    lines = [
        "# Tapping evaluation",
        "",
        f"Software: {pipeline}. Reference: tap times marked by a person at the moment the "
        "fingertips touch.",
        "",
        "## Summary",
        "",
        f"- Clips: {s.clips} ({s.failures} could not be analysed)",
        f"- Marked taps: {s.marked_taps}; recall {s.recall:.1%}, precision {s.precision:.1%}",
        f"- Tap count error per clip (mean absolute): {s.count_error_mean_abs:.2f}",
        f"- Rate error (mean absolute / worst): {s.rate_error_mean_abs_percent:.2f}% / "
        f"{s.rate_error_worst_percent:.2f}%",
        f"- Tap time error: bias {s.timing_bias_ms:+.1f} ms, median absolute "
        f"{s.timing_error_median_abs_ms:.1f} ms, 95th percentile "
        f"{s.timing_error_p95_abs_ms:.1f} ms",
        f"- Hesitation count agrees on {s.hesitation_agree} of {len(scores)} clips; "
        f"arrest count on {s.arrest_agree}",
        "",
        "## Against the proposed limits",
        "",
        *[f"- {'MET' if ok else 'NOT MET'}: {name}" for name, ok in verdicts(s)],
        "",
        "## Clips",
        "",
        "| Clip | Marked | Found | Missed | Extra | Rate error | Bias ms |",
        "|---|---|---|---|---|---|---|",
    ]
    for c in scores:
        bias = 1000 * float(np.mean(c.timing_errors)) if c.timing_errors else float("nan")
        lines.append(
            f"| {c.video} | {c.marked_count} | {c.found_count} | {c.missed} | {c.extra} "
            f"| {c.rate_error_percent:+.1f}% | {bias:+.1f} |"
        )
    unmatched = [c for c in scores if c.missed_times or c.extra_times]
    for c in unmatched:
        if c is unmatched[0]:
            lines += ["", "## Taps that did not match", ""]
        lines.append(
            f"- {c.video}: marked but not found at "
            f"{', '.join(f'{t:.2f} s' for t in c.missed_times) or 'none'}; found but not marked at "
            f"{', '.join(f'{t:.2f} s' for t in c.extra_times) or 'none'}"
        )
    for f in failures:
        if f is failures[0]:
            lines += ["", "## Clips the software could not analyse", ""]
        lines.append(f"- {f.video}: {f.reason}")
    lines += [
        "",
        "Amplitude, variability and decrement are not in this table: a person cannot mark them "
        "by eye, so they need a separate reference (see docs/EVALUATION.md).",
        "",
    ]
    return "\n".join(lines)


def to_json(scores: list[ClipScore], failures: list[Failure], pipeline: str) -> str:
    """The same results as machine-readable JSON."""
    return json.dumps(
        {
            "pipeline": pipeline,
            "summary": asdict(summarise(scores, failures)),
            "clips": [asdict(c) for c in scores],
            "failures": [asdict(f) for f in failures],
        },
        indent=2,
    )
