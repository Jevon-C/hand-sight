# Hand Sight

Objective, repeatable measures from hand videos, to support the hand items of the MDS-UPDRS in
Parkinson's disease (ALS is exploratory). Upload a video, choose a test, and get timing and
amplitude measures with the tracked hand drawn over the video. Everything runs on one computer,
in the browser.

> **Research prototype, not a medical device.** Measures are labelled *illustrative* until their
> error has been measured against hand-marked clips (docs/EVALUATION.md). Do not use them for
> diagnosis or treatment decisions.

## Start here (a five-minute tour for reviewers)

If you only read a few things, read these. Each shows a different part of the engineering.

1. **Geometry core, with property tests:** `crates/geom`. A camera model with lens distortion, pose from
   a flat board, RANSAC plane fitting, rigid alignment and a Kalman and RTS smoother. The tests
   (for example `camera.rs` and `align.rs`) generate random inputs and check round trips, such as
   project then unproject, and a fixed reprojection-error ceiling.
2. **Rust and Python parity:** `crates/hand/tests/parity.rs` and `python/tests/test_parity_fixture.py`.
   The Rust hand tracker is tested against the Python one on shared fixtures (letterboxing,
   anchors, palm decoding, affine crops, resizing), and on the laptop it matches to 0.000000 px
   over 120 real frames. Background: docs/RUST-PORT.md.
3. **Edge speed and the int8 decision:** docs/DECISIONS.md, D15 (speed and quantisation results),
   D22 (why the CPU is the default device). On the NPU the hand model takes about 9 ms per frame
   against about 39 ms on the CPU; an int8 version was measured and rejected because its error
   was too large. Tools: `python/src/axonhand/tools/benchmark.py`, `quantize.py`.
4. **Reproducible results:** docs/STORAGE.md. Each result is keyed by the video, the algorithm
   version, the settings and the model weights, written once and never changed, and checked for
   tampering when read.
5. **Continuous integration:** `.github/workflows/ci.yml` and the Actions tab. Lint, strict typing,
   Rust and Python tests and a WebAssembly build on Linux and Windows, plus a browser end-to-end
   test (`e2e/test_browser.py`).

What is not done is stated plainly under "Status and evidence" below.

## What it does

- **Analysis:** pick a test, pick or upload a video, and see the measures, the timeline of every
  tap, and the video with the tracked hand overlaid. The clip is stepped frame by frame from the keyboard.
- **Comparison:** set two results side by side with the differences, and reasons for care
  (different test version, model or settings; same video).
- **Library:** the stored videos, tests and results; download any result as CSV.
- **Reproducible:** a result is keyed by the video, the test version, the settings and the model,
  stored once and never changed, so the same inputs always return the same result.
- **Extensible:** a new test is a folder with a manifest and a short Python file; no change to
  the core (docs/PLUGIN-AUTHORING.md). Four tests are included: finger tapping, hand open-close,
  pronation-supination and postural tremor.
- **Private by default:** the server listens on `127.0.0.1` only and keeps data in a local folder.

## How it is built

Rust throughout the server and browser app; Python for the test plug-ins.

| Part | Where | Notes |
| --- | --- | --- |
| Server | `crates/server` | Axum; uploads, job queue, result store, CSV export |
| Browser app | `crates/ui` | Leptos compiled to WebAssembly, built with Trunk |
| Shared API types | `crates/api` | One definition of every request and response; OpenAPI file generated |
| Geometry core | `crates/geom`, `crates/geom-py` | Camera model, alignment, plane fit, smoothing; Python bindings |
| Hand model | `crates/hand`, `crates/track`, `python/src/axonhand/hand` | Two-stage ONNX hand tracker in Rust and Python, which match; NPU or CPU |
| Tests (plug-ins) | `plugins/` | Manifest plus Python entry point per test |
| Analysis tools | `python/` | Metrics, synthetic ground truth, evaluation, benchmark |

More: docs/ARCHITECTURE.md, docs/STORAGE.md, docs/PLUGIN-PROTOCOL.md, docs/METRICS.md.

## Quick start (Windows, ARM64 or x64)

1. Install Rust and LLVM (put `C:\Program Files\LLVM\bin` on PATH), and Python 3.12.
2. From the repo folder:

```powershell
py -3.12 -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -e "python[dev]"
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
powershell -ExecutionPolicy Bypass -File scripts\dev.ps1
```

3. Open http://127.0.0.1:8080.

Without the hand-model files in `models/` (`scripts\fetch_models.ps1`), the app runs with a
development stand-in tracker so you can try every screen; its numbers are marked illustrative.

### Hand model choice

`AXONHAND_HAND_MODEL=auto` (the default) uses the Rust tracker when it can (`scripts\dev.ps1`
builds it), then the Python pipeline, then the stand-in; the log says which and why.
`mediapipe_rust` demands the Rust tracker (`cargo build --release -p axonhand-track`; the binary next
to the server is found automatically, or set `AXONHAND_TRACK`); `mediapipe` forces Python. On the
NPU ("Run on the NPU" in the set-up panel) positions differ slightly from the CPU, so the CPU is the
default (docs/DECISIONS.md D22). Compare the two trackers on a video with
`scripts\try_rust_tracker.ps1 -Video C:\path\clip.mp4`; details in docs/RUST-PORT.md.

## Checks

```powershell
powershell -File scripts\check.ps1          # Windows
scripts/check.sh                            # Linux and macOS
powershell -ExecutionPolicy Bypass -File scripts\e2e.ps1   # browser test (needs Trunk and a browser)
```

`check` runs ruff, mypy (strict), pytest, rustfmt, clippy (warnings are errors), the Rust tests and a
WebAssembly build check. CI (`.github/workflows/ci.yml`) runs the same on Linux and Windows and
the browser test on Linux. Optional: `pip install pre-commit` then `pre-commit install` for fast
checks on each commit.

## Status and evidence

Built and tested: the full app, four test plug-ins validated on synthetic hands with known
ground truth, a hand tracker that matches between Rust and Python, and speed measurements on the
development laptop (NPU about 9 ms per frame against 39 ms on CPU).

Not yet done: error measured against hand-marked real clips (the tools and protocol exist,
docs/EVALUATION.md), board detection and scale, literature sign-off for each measure
(docs/METRICS.md). Milestones and open items: docs/PLAN.md. Why things are the way they are:
docs/DECISIONS.md.

## Repository map

`crates/` Rust · `python/` Python package · `plugins/` tests · `scripts/` run, check, capture ·
`e2e/` browser test and screenshots · `docs/` design and decisions · `spike/` early probes of what
runs on Arm Windows (history, see spike/README.md).

## Licence

MIT, see LICENSE. Third-party material and its licences: THIRD-PARTY-NOTICES.md. Contributions: see CONTRIBUTING.md. Security and data handling: SECURITY.md.
