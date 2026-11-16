"""How closely two people agree when they mark the same clip.

The software cannot be judged more exactly than people agree with each other, so this gives the
floor for the timing limit in ``evaluation.report``. It uses the same matching rule as the
comparison with the software, with the first marker as the reference.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from axonhand.evaluation.scoring import match_taps, tolerance_seconds


@dataclass(frozen=True)
class Agreement:
    """Two markers on one clip. Timing errors are the second marker minus the first, in seconds."""

    video: str
    first_count: int
    second_count: int
    hits: int
    only_first: tuple[float, ...]
    """Times marked by the first person only."""
    only_second: tuple[float, ...]
    """Times marked by the second person only."""
    timing_errors: tuple[float, ...]
    tolerance: float


def compare_marks(
    video: str, first: tuple[float, ...], second: tuple[float, ...], fps: float
) -> Agreement:
    """Compare two sets of marked times for one clip.

    Raises:
        ValueError: if the first marker has fewer than four marks (no tolerance can be set).
    """
    reference = np.asarray(first, dtype=np.float64)
    other = np.asarray(second, dtype=np.float64)
    if reference.size < 4:
        raise ValueError("at least four marks are needed from the first marker")
    tolerance = tolerance_seconds(reference, fps)
    matching = match_taps(other, reference, tolerance)
    return Agreement(
        video=video,
        first_count=int(reference.size),
        second_count=int(other.size),
        hits=len(matching.found_index),
        only_first=tuple(
            float(t) for j, t in enumerate(reference) if j not in matching.marked_index
        ),
        only_second=tuple(float(t) for i, t in enumerate(other) if i not in matching.found_index),
        timing_errors=tuple(float(e) for e in matching.errors),
        tolerance=tolerance,
    )


@dataclass(frozen=True)
class AgreementSummary:
    """Pooled over clips."""

    clips: int
    marks: int
    agreement: float
    """Matched marks as a fraction of all marks by either person (Jaccard-style)."""
    bias_ms: float
    median_abs_ms: float
    p95_abs_ms: float


def summarise(items: list[Agreement]) -> AgreementSummary:
    """Pool the clips."""
    errors = np.array([e for a in items for e in a.timing_errors], dtype=np.float64) * 1000.0
    hits = sum(a.hits for a in items)
    union = sum(a.first_count + a.second_count - a.hits for a in items)
    return AgreementSummary(
        clips=len(items),
        marks=sum(a.first_count for a in items),
        agreement=hits / union if union else float("nan"),
        bias_ms=float(errors.mean()) if errors.size else float("nan"),
        median_abs_ms=float(np.median(np.abs(errors))) if errors.size else float("nan"),
        p95_abs_ms=float(np.percentile(np.abs(errors), 95)) if errors.size else float("nan"),
    )


def markdown(items: list[Agreement]) -> str:
    """The comparison as Markdown."""
    s = summarise(items)
    lines = [
        "# Agreement between two markers",
        "",
        f"- Clips: {s.clips}; marks by the first person: {s.marks}",
        f"- Marks both people made (within tolerance): {s.agreement:.1%} of all marks",
        f"- Time difference (second minus first): bias {s.bias_ms:+.1f} ms, median absolute "
        f"{s.median_abs_ms:.1f} ms, 95th percentile {s.p95_abs_ms:.1f} ms",
        "",
        "The software cannot be judged more exactly than this. A timing limit tighter than the "
        "95th percentile here would measure the markers, not the software.",
        "",
        "| Clip | First | Second | Both | Only first | Only second |",
        "|---|---|---|---|---|---|",
    ]
    for a in items:
        lines.append(
            f"| {a.video} | {a.first_count} | {a.second_count} | {a.hits} "
            f"| {', '.join(f'{t:.2f}' for t in a.only_first) or '-'} "
            f"| {', '.join(f'{t:.2f}' for t in a.only_second) or '-'} |"
        )
    lines.append("")
    return "\n".join(lines)
