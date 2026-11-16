# Extensibility design (decision: scalable = extensible + reproducible)

Decided: "scalable for new information" means (1) new tests/metrics as plug-ins and
(2) new models/data as versioned artifacts. Throughput scaling (queues, many sites) is
deferred behind a clean job interface.

## 1. Plug-ins (tests and metrics)
A test is a Python module exposing:
- `manifest`: declarative description. id, version, display name, required inputs
  (e.g. hand landmarks, objects), metrics (name, unit, definition, valid range),
  quality rules (min fps, hand visibility, framing), and result layout hints.
- `run(inputs, params) -> Result`: pure function, no I/O, deterministic given seed.
The Rust server loads manifests from the registry and builds the upload options and
results screen from them. Adding a test = adding a module + manifest; core and UI untouched.
Manifests are validated against a schema at load; invalid plug-ins are rejected with a
clear error, never half-loaded.

## 2. Versioned artifacts (models and data)
Every result is immutable and keyed by:
  sha256(video) + test id@version + canonical params + weights hash (+ calibration id).
Retraining or changing an algorithm produces new results beside the old ones; nothing is
overwritten. Models ship with a model card (data, metrics, licence, intended use).
Longitudinal comparison only joins results with compatible keys, and flags mismatches.

## Exit criteria
- M3: the finger-tapping and reach-to-grasp tests are registered through this interface,
  and a third dummy test is added in a test without modifying core code.
- M7: UI options are generated from manifests; deleting a plug-in removes it from the UI.
- Any milestone producing results: re-running the same key returns byte-identical output.
