"""Compare ways of smoothing the opening signal on synthetic tapping with known truth.

Usage::

    python -m axonhand.tools.smoothing_study [--out DIR]

Generates tapping at several rates, frame rates and noise levels, runs the tap analysis with each
smoother and reports how far the tap times, the amplitudes and the contact times are from the
truth. Writes ``smoothing.md`` to ``--out`` (default ``data/smoothing``).

The synthetic noise is white and the same size in every frame; real tracking error is not (it
comes in bursts, e.g. when a fingertip is hidden), so a smoother that wins here has not been shown
to win on real hands.
"""

from __future__ import annotations

import argparse
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from numpy.typing import NDArray

from axonhand.geometry import smooth_track
from axonhand.synthetic import TapSpec, generate_tapping
from axonhand.tapping import AnalysisError, Smoother, _smooth, analyse

RATES = (1.0, 2.0, 3.5)
FPS = (30.0, 60.0)
NOISES = (0.0, 0.01, 0.02, 0.04)
SEEDS = (0, 1, 2)


def moving_average(signal: NDArray[np.float64]) -> NDArray[np.float64]:
    """The current smoother: a three-point average."""
    return _smooth(signal)


def identity(signal: NDArray[np.float64]) -> NDArray[np.float64]:
    """No smoothing."""
    return signal


def kalman(fps: float, acceleration_std: float, noise_std: float) -> Smoother:
    """The geometry core's constant-velocity Kalman filter with a backward (RTS) pass."""

    def apply(signal: NDArray[np.float64]) -> NDArray[np.float64]:
        smoothed = smooth_track(
            signal.tolist(), 1.0 / fps, acceleration_std**2, max(noise_std, 1e-4) ** 2
        )
        return np.asarray(smoothed, dtype=np.float64)

    return apply


@dataclass(frozen=True)
class Row:
    """Errors of one smoother at one noise level, pooled over the runs."""

    smoother: str
    noise: float
    runs: int
    failures: int
    peak_time_ms: float
    contact_time_ms: float
    amplitude_percent: float
    count_errors: int


def study(noise: float, fps_list: tuple[float, ...] = FPS) -> list[Row]:
    """Run every smoother over the synthetic grid at one noise level."""
    rows: list[Row] = []
    for name, build in _smoothers():
        peak_err: list[float] = []
        contact_err: list[float] = []
        amp_err: list[float] = []
        failures = 0
        count_errors = 0
        runs = 0
        for fps in fps_list:
            for rate in RATES:
                for seed in SEEDS:
                    runs += 1
                    data = generate_tapping(
                        TapSpec(
                            rate_hz=rate, taps=int(10 + 4 * rate), fps=fps, noise=noise, seed=seed
                        )
                    )
                    # distance noise is about sqrt(2) times a landmark's, in palm lengths
                    smoother = build(fps, 4.0 * rate * rate * 2 * np.pi, noise * 1.5)
                    try:
                        result = analyse(data.track, smoother=smoother)
                    except AnalysisError:
                        failures += 1
                        continue
                    truth = data.truth
                    if result.tap_count != truth.tap_count:
                        count_errors += 1
                        continue
                    peak_err.extend(np.abs(result.peak_times - truth.peak_times) * 1000.0)
                    period = 1.0 / rate
                    expected = truth.peak_times + period / 2
                    nearest = np.abs(result.contact_times[None, :] - expected[:, None]).min(axis=1)
                    contact_err.extend(np.minimum(nearest, period / 2) * 1000.0)
                    amp_err.extend(
                        100.0 * np.abs(result.amplitudes - truth.amplitudes) / truth.amplitudes
                    )
        rows.append(
            Row(
                name,
                noise,
                runs,
                failures,
                float(np.mean(peak_err)) if peak_err else float("nan"),
                float(np.mean(contact_err)) if contact_err else float("nan"),
                float(np.mean(amp_err)) if amp_err else float("nan"),
                count_errors,
            )
        )
    return rows


Builder = Callable[[float, float, float], Smoother]
"""Makes a smoother from the frame rate, an acceleration scale and the noise size."""


def _smoothers() -> list[tuple[str, Builder]]:
    def plain(builder: Smoother) -> Builder:
        return lambda fps, a, n: builder

    return [
        ("none", plain(identity)),
        ("3-point average", plain(moving_average)),
        ("Kalman (acceleration scaled to rate)", lambda fps, a, n: kalman(fps, a, n)),
        ("Kalman (4x looser)", lambda fps, a, n: kalman(fps, 4.0 * a, n)),
    ]


def markdown(rows: list[Row]) -> str:
    """The results as a Markdown table."""
    lines = [
        "# Smoothing study (synthetic tapping, known truth)",
        "",
        "Errors are means over all taps. The taps per run are 14 to 24; rates 1, 2 and 3.5 per "
        "second; 30 and 60 frames per second; 3 random seeds. Noise is the standard deviation of "
        "each landmark's position in palm lengths. Contact time is the closing of the fingers; "
        "its truth is the end of each synthetic cycle, and the definition lets it come earlier "
        "(within the last 10% of the way down), so a constant offset of a few tens of "
        "milliseconds is expected and the comparison between smoothers is what matters.",
        "",
        "| Noise | Smoother | Runs failed | Wrong tap count | Peak time error ms "
        "| Contact time error ms | Amplitude error % |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in rows:
        lines.append(
            f"| {r.noise:.2f} | {r.smoother} | {r.failures} | {r.count_errors}/{r.runs} "
            f"| {r.peak_time_ms:.1f} | {r.contact_time_ms:.1f} | {r.amplitude_percent:.1f} |"
        )
    lines.append("")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("data/smoothing"))
    args = parser.parse_args(argv)
    rows: list[Row] = []
    for noise in NOISES:
        rows.extend(study(noise))
    args.out.mkdir(parents=True, exist_ok=True)
    text = markdown(rows)
    (args.out / "smoothing.md").write_text(text, encoding="utf-8")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
