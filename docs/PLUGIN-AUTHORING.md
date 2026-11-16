# Writing a test plug-in

A new test is a folder with a manifest and one function. Start from the template.

## 1. Copy the template

Copy `plugins/_template/` to `plugins/<your_id>/` and edit `plugin.toml`. The folder name must
equal `id` (lowercase letters, digits and `_`).

## 2. Describe the test in `plugin.toml`

Say what the test is (`name`, `clinical_item`, `summary`), how to film it (`[capture]`), which
numbers it reports (`[[metrics]]`) and which settings a person may change (`[[parameters]]`).
Set `status = "planned"` and leave out `entrypoint` to list a test that is not built yet. The
full key list is in `PLUGIN-PROTOCOL.md`.

## 3. Write the function

```python
from axonhand.plugin import Job, Measure, PluginError, PluginResult, run


def compute(job: Job) -> PluginResult:
    if not job.video_path.is_file():
        raise PluginError("The video could not be found.")
    rate = 3.2 * job.parameters["scale"]
    return PluginResult(
        measures=[Measure(metric, rate) for metric in job.metrics],
        notes=["Optional remarks for the clinician."],
    )


if __name__ == "__main__":
    raise SystemExit(run(compute))
```

- Return one `Measure` for every metric in `job.metrics`, and no others.
- When a number cannot be measured on this video, return `Measure(id, None, "reason")`. Do not
  invent a value.
- Raise `PluginError` with a sentence for the clinician when the whole run cannot continue.
  Never put file paths or personal data in it. Any other exception is shown only as a generic
  failure; its traceback goes to the log.
- Values must be finite. NaN and infinity are refused.

## 4. Check it

1. Start the app (`scripts/dev.ps1`). Your plug-in appears on the Tests screen. A mistake in
   the manifest shows there as an error with the reason.
2. Test the function directly with pytest; see `python/tests/test_plugin.py` for the pattern.
3. Run `scripts/check.ps1` before committing.

## Worked example

`plugins/example_fixed_values/` is a complete plug-in that returns made-up numbers. Its numbers
mean nothing; it exists to show the format.
