"""Write a synthetic finger-tapping video, for end-to-end tests.

Usage: ``python -m axonhand.tools.make_test_video OUT.mp4 [--taps N] [--rate HZ] [--decrement D]``
Prints the generator's truth as JSON on stdout.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from axonhand.synthetic import TapSpec, generate_tapping, render_video


def main(argv: list[str] | None = None) -> int:
    """Run the tool; returns the exit status."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--taps", type=int, default=12)
    parser.add_argument("--rate", type=float, default=2.5)
    parser.add_argument("--decrement", type=float, default=0.0)
    args = parser.parse_args(argv)
    try:
        spec = TapSpec(taps=args.taps, rate_hz=args.rate, decrement=args.decrement)
        synthetic = generate_tapping(spec)
        render_video(synthetic.track, args.output)
    except (ValueError, OSError) as error:
        print(str(error), file=sys.stderr)
        return 2
    truth = synthetic.truth
    print(json.dumps({"tap_count": truth.tap_count, "tap_rate": truth.rate_hz}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
