"""Tap times marked by a person on a video: the reference the software is measured against.

A tap is marked at the frame where the thumb and index fingertips *touch* as the fingers close.
That moment is sharp, so people agree on it; the software finds the same moment from the
opening signal (``tapping.contact_frames``, ``docs/METRICS.md``). The file sits next to the video
as ``<video>.taps.json``.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path

SUFFIX = ".taps.json"
_VERSION = 2


class AnnotationError(Exception):
    """An annotation file is missing, damaged or inconsistent."""


@dataclass(frozen=True)
class TapAnnotation:
    """Tap times of one video.

    Attributes:
        video: file name of the video the times belong to.
        times: seconds from the start of the video, strictly increasing.
        annotator: who marked them (free text), so two people's marks can be compared.
        note: anything worth knowing about the clip (lighting, a tremor, a hand hidden).
    """

    video: str
    times: tuple[float, ...]
    annotator: str = ""
    note: str = ""
    version: int = field(default=_VERSION, compare=False)

    def __post_init__(self) -> None:
        if not self.video:
            raise AnnotationError("the video name is empty")
        previous = -1.0
        for time in self.times:
            if not time >= 0 or not time > previous:
                raise AnnotationError("tap times must be non-negative and strictly increasing")
            previous = time

    def to_json(self) -> str:
        """The file content."""
        return json.dumps(
            {
                "version": self.version,
                "video": self.video,
                "annotator": self.annotator,
                "note": self.note,
                "tap_times": [round(t, 4) for t in self.times],
            },
            indent=2,
        )

    @staticmethod
    def from_json(text: str) -> TapAnnotation:
        """Parse file content.

        Raises:
            AnnotationError: if it is not a tap annotation of a known version.
        """
        try:
            data = json.loads(text)
            if data["version"] == 1:
                raise AnnotationError(
                    "this file marks the widest opening; marks are now made at the moment the "
                    "fingertips touch, so the clip needs marking again"
                )
            if data["version"] != _VERSION:
                raise AnnotationError(f"unknown annotation version {data['version']}")
            return TapAnnotation(
                video=str(data["video"]),
                times=tuple(float(t) for t in data["tap_times"]),
                annotator=str(data.get("annotator", "")),
                note=str(data.get("note", "")),
            )
        except (ValueError, KeyError, TypeError) as error:
            raise AnnotationError(f"not a tap annotation file: {error}") from error


def annotation_path(video: Path) -> Path:
    """Where the annotation of ``video`` is kept."""
    return video.with_name(video.name + SUFFIX)


def save(video: Path, annotation: TapAnnotation) -> Path:
    """Write the annotation next to its video and return the file path."""
    path = annotation_path(video)
    path.write_text(annotation.to_json(), encoding="utf-8")
    return path


def load(video: Path) -> TapAnnotation | None:
    """Read the annotation of ``video``; ``None`` if it has none.

    Raises:
        AnnotationError: if the file exists but cannot be used.
    """
    path = annotation_path(video)
    if not path.is_file():
        return None
    return TapAnnotation.from_json(path.read_text(encoding="utf-8"))
