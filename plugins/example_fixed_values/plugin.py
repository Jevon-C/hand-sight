"""Example plug-in: returns fixed values without reading the video."""

from axonhand.plugin import Job, Measure, PluginResult, run


def compute(job: Job) -> PluginResult:
    """Return fixed numbers for whichever metrics were requested."""
    values = {
        "example_rate": 2.5 * job.parameters.get("multiplier", 1.0),
        "example_count": 12.0,
    }
    return PluginResult(
        measures=[Measure(metric, values[metric]) for metric in job.metrics],
        notes=["These numbers are made up. This plug-in does not look at the video."],
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
