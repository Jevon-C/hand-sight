"""Template plug-in. Replace the body of ``compute`` with the real analysis."""

from axonhand.plugin import Job, Measure, PluginError, PluginResult, run


def compute(job: Job) -> PluginResult:
    """Return one measure for each requested metric."""
    if not job.video_path.is_file():
        raise PluginError("The video could not be found.")
    return PluginResult(
        measures=[Measure(metric, None, "Not implemented yet.") for metric in job.metrics]
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
