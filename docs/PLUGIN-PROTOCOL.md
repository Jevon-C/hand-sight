# Plug-in protocol

A test (finger tapping, tremor, and so on) is a **plug-in**: a folder under `plugins/` that the
server runs as a separate process. This page is the contract between the two. The Python helpers
in `axonhand.plugin` implement the plug-in side; see `PLUGIN-AUTHORING.md` to write one.

## Why a separate process

A crash, a hang or runaway output in analysis code must not take the server down, and analysis
code must not be able to read the server's memory. The server therefore never imports plug-in
code. It starts a process, waits under limits, and checks the answer.

## Folder layout

```
plugins/<id>/plugin.toml     manifest (required)
plugins/<id>/<entrypoint>    script the server runs (not for planned tests)
```

The folder name must equal the manifest `id`, which keeps ids unique. Folders starting with `.`
or `_` are ignored (`_template` is a starting point for authors).

## Manifest (`plugin.toml`)

Parsed strictly: an unknown key is an error. Every problem is reported at once.

| Key | Meaning |
| --- | --- |
| `id` | Lowercase letter first, then lowercase letters, digits, `_`; at most 48 characters |
| `version` | `major.minor.patch`, digits only |
| `name`, `clinical_item`, `summary` | Text shown to people; not empty |
| `status` | `available` (can run) or `planned` (listed, read-only) |
| `entrypoint` | Relative path of the script, inside the folder. Required for `available`, forbidden for `planned` |
| `[capture]` | `min_fps`, `min_duration_seconds` (positive), `board_required` (optional), `instructions` |
| `[[metrics]]` | `id`, `name`, `unit` (optional), `description`, `default_selected` (optional). Ids unique |
| `[[parameters]]` | `id`, `name`, `description`, `kind` (`number`, `integer`, `boolean`), `default`, optional `min`, `max`. Ids unique; default inside the range; integer defaults whole; boolean defaults 0 or 1 |

A plug-in that fails validation is still listed, with status `broken` and every reason, so a
mistake is visible in the app. It never affects other plug-ins.

## Running a job

The server creates a work folder and runs

```
<python> <plugin folder>/<entrypoint> <work>/job.json <work>/result.json
```

with the work folder as the current directory, no input, and standard output and error captured
to `output.log`. Environment variables are inherited; `PYTHONUTF8=1` is set.

### `job.json`

```json
{
  "schema_version": 1,
  "job_id": "…",
  "test_id": "finger_tapping",
  "video_path": "/absolute/path/to/video.mp4",
  "metrics": ["tap_rate", "amplitude"],
  "parameters": { "min_amplitude": 0.1 },
  "landmarks_path": "/absolute/path/to/landmarks.npz"
}
```

`landmarks_path` is present when the test works from hand landmarks (all the movement tests do).
The server runs the hand model first and writes the landmarks there; the file holds `fps` and
`points`, an array of shape frames × 21 × 3 in palm lengths (`NaN` where a landmark was not found).
A plug-in reads it with `axonhand.landmarks.LandmarkTrack.load`.

`parameters` always holds every setting of the test: the person's choice, or the default. Values
are already checked against the manifest (range, whole numbers, booleans as 0 or 1).

### `result.json`

```json
{
  "schema_version": 1,
  "measures": [
    { "metric_id": "tap_rate", "value": 3.2 },
    { "metric_id": "amplitude", "value": null, "reason": "No taps were found." }
  ],
  "quality": [
    { "name": "Tap count", "status": "flag", "detail": "Only 6 taps; rates are less reliable." }
  ],
  "notes": ["Hand partly out of frame near the end."]
}
```

`quality` is optional. Each check has a `status` of `pass`, `flag` (read with care) or `fail`
(do not rely on the numbers). They are saved with the result and shown beside it.

Unknown keys are rejected. The server refuses a result when the schema version is wrong, a
metric was not requested or appears twice, a requested metric is missing, a value is not finite,
or a missing value has no reason.

### `signals.json` (optional)

Data for the result's plots, written next to `result.json`:

```json
{
  "schema_version": 1,
  "series": [
    { "id": "opening", "name": "Thumb to index distance", "unit": "palm lengths",
      "start_seconds": 0.0, "step_seconds": 0.0166, "values": [0.0, 0.1] }
  ],
  "events": [
    { "id": "taps", "name": "Taps", "unit": "palm lengths", "times": [0.5, 0.9], "values": [0.8, 0.79] }
  ]
}
```

Steps must be positive, every number finite, and an event list needs one value per time.
A file that fails these checks is refused and the run fails with that reason.
A series and an event list with the same `unit` are drawn together.

### Failure

Exit with a non-zero status. To tell the user why, first write `error.json`
(`{"error": "No hand was visible in the video."}`); the first 500 characters, without control
characters, are shown. Without it the user sees only the exit status. Anything printed to the
log is for developers and is never sent to the browser.

## Limits

| Limit | Default | Outcome |
| --- | --- | --- |
| Run time | 10 minutes | process killed, "took too long" |
| Size of `result.json` and `output.log` | 8 MiB each | process killed, "too much output" |

## Versions

`schema_version` is 1. A change that old plug-ins cannot ignore raises it, and the server refuses
results with a version it does not know.
