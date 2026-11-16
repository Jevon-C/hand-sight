# Hand Sight build plan

Goal: upload hand videos, pick a test and metrics, get objective measures for Parkinson's
(MDS-UPDRS hand items) with ALS as an exploratory module. Rust + Python only. Runs on a
Surface ARM laptop (edge). Training on GCP. Portfolio focus: 3D perception, custom neural
networks for simulation/robotics-style problems, reproducible engineering.

Principle: every milestone ends with a measurable exit criterion. Do not start the next
milestone until the current one passes.

## Milestones

| # | Milestone | Exit criterion |
|---|-----------|----------------|
| M0 | Foundations + ARM spike | `scripts/check` green locally and in CI; `spike-report.md` read; runtime choice recorded in docs/DECISIONS.md (done: ort + QNN NPU) |
| M1 | Rust geometry core | Camera model, DLT triangulation, PnP, RANSAC plane fit, Kabsch, Kalman/RTS smoother; property tests; reprojection error ceiling enforced in tests |
| M2 | Synthetic generator | Parametric hand/object motion with ground truth (tapping, open-close, reach-to-grasp) rendered as 2D/3D keypoints and optionally images |
| M3 | Metric engine + plug-in registry | Finger-tapping and reach-to-grasp registered as plug-ins (manifest + run); a dummy third test added without touching core; metrics recover known synthetic parameters within a stated percentage; see DESIGN-extensibility.md |
| M4 | Detector on GCP | Multi-class detector (cup, pen, coin, ball, towel, key) trained on GCP, int8 QDQ ONNX, runs in Rust (`ort`) on the laptop NPU; mAP, latency and fraction of nodes on NPU recorded |
| M5 | Temporal hand-state network | Custom net (kinematic-constrained, uncertainty output) beats the non-learned baseline in ablations; measured sim-to-real gap on a small real set |
| M6 | Edge benchmark | Latency / memory / model-size table on the ARM laptop (CPU vs NPU) vs budgets set after baseline |
| M7 | Web staging + plug-in registry | Axum + Leptos: upload, staging area, choose test/metrics, results; new test added by registering a plug-in without touching the core |
| M8 | Portfolio write-up | Benchmark table, model cards, ablations, demo video, honest limitations |

Scalability decision: extensible plug-ins + versioned, immutable results (docs/DESIGN-extensibility.md). Throughput scaling deferred.

## M1 progress
Complete (2026-11-12): camera model with distortion, rigid alignment, plane fit with RANSAC, pose from a flat board, Kalman/RTS smoother, and Python bindings with a reprojection-ceiling test. 30 Rust tests and 10 Python tests pass; the full check script is green. Triangulation is deferred (DECISIONS D7). Remaining Phase 0 items are tracked in the master checklist.

## Phase 4 progress
End to end with a stand-in hand model (2026-11-12): upload, choose a test, run a job, see a result with plots, download CSV. The result key, write-once result store, job queue, API, finger-tapping analysis, synthetic ground truth and an end-to-end test are done. Added (2026-11-13): the tracked hand drawn over the video (from `landmarks.json`, saved with each result by the real hand model), a keyboard-controlled player (space, arrows for one frame, Shift for ten, O for the overlay, [ and ] for speed), a results list endpoint (`GET /api/results`) and a Compare screen that sets two results of the same test side by side with the differences and reasons for care (different test version, model or settings; same video). The browser parts compile and are linted and unit-tested natively but can only be seen by building the UI with trunk on the laptop. Browser end-to-end test written (2026-11-13, `scripts/e2e.ps1`, `e2e/test_browser.py`): upload, analyse, overlay, keyboard, Compare; the development stand-in tracker now also writes the overlay file so the test needs no real model. Clinician score field done (2026-11-13): `GET/PUT /api/results/{key}/scores`, a score card on the Results page, per-rater scores kept in the index (docs/STORAGE.md). The Compare screen lines the scores up by rater (a dash where a rater scored only one result; a change only where the same rater scored both). Not yet: using the scores in the evaluation report (agreement between the measures and the scores).

## Phase 5 progress
In progress (2026-11-13). Built: ONNX loading with NPU-or-CPU choice, the two-stage pipeline with tracking, video reading with rotation, the `hand_model` command, the landmark drawing tool, and the server choosing the real model when its files exist. The real model runs on the NPU (about 117 frames per second on the laptop). Evaluation tools built: `axonhand.tools.annotate` (mark taps in the browser) and `axonhand.evaluate` (error report), see docs/EVALUATION.md. Also built (2026-11-13, run on the laptop to get numbers): `axonhand.compare_markers`, `axonhand.tools.benchmark` (speed table), `axonhand.tools.quantize` (int8 landmark model and its error), `axonhand.tools.smoothing_study`; the literature check is in docs/METRICS.md. Benchmark and quantisation run on the laptop (2026-11-13): NPU 9.3 ms per frame (107 fps) against 39.3 ms on CPU; int8 not adopted (DECISIONS D15). Still to do: record and mark 10 or more clips and run the report (paused), board detection and scale. Rust hand crate built (2026-11-13, docs/RUST-PORT.md): geometry, Pillow-exact image operations and both models match Python in the sandbox; real-clip parity confirmed on the laptop (0.000000 px over 120 frames); NPU placement from Rust measured (2 to 3 ms per frame, landmarks within limits of CPU, 120 frames). Tracker program `axonhand-track` and server option `AXONHAND_HAND_MODEL=mediapipe_rust` built (2026-11-13, DECISIONS D21); compared on the laptop (2026-11-13): on the CPU identical to Python in all 367 frames of a second clip, on the NPU close but not within the proposed limits; so the device is now chosen per analysis, CPU by default (DECISIONS D22); the automatic hand model choice now prefers the Rust tracker, and the Compare screen shows the landmark difference between two results of one video (D23).

Hand test plug-ins (2026-11-13): hand_open_close, pronation_supination and postural_tremor written and available (docs/METRICS.md, D17), validated on synthetic hands only; still to do: literature check, real clips, hand-marked checks, a tremor noise measurement on a still hand.

## M0 status
Done: ARM64 spike (4 rounds), runtime decision (docs/DECISIONS.md D2), skeleton passing its checks.
Remaining: repo name and licence, single or two-view capture, GCP project and budget alerts,
choice of ONNX hand-landmark model (licence and accuracy check). M1 can start in parallel.

## Laptop environment (fixed by the spike)
- Rust 1.99, target aarch64-pc-windows-msvc; LLVM clang at C:\Program Files\LLVM\bin on PATH.
- Python 3.12 ARM64 (not 3.15). Working: numpy, scipy, onnx, onnxruntime, onnxruntime-qnn, PyAV,
  pillow, polars, maturin, PyTorch CPU (add `--extra-index-url https://download.pytorch.org/whl/cpu`).
- Not installable natively: opencv, pyarrow, mujoco. mediapipe: wheel exists but imports cv2, so it fails (spike round 5). Do not depend on these on the laptop.
- NPU: register onnxruntime-qnn, then `add_provider_for_devices`; int8 QDQ models preferred.

## GCP from scratch checklist
1. Create a Google Cloud account and project; attach billing.
2. Set a budget with alerts (e.g. 50/90/100%) BEFORE anything else.
3. Request GPU quota (new projects often have zero) for one region; use spot GPU VMs.
4. Create a Cloud Storage bucket for datasets and checkpoints; keep data hashes.
5. Containerise training (Dockerfile + lockfile); seed everything; log configs and weights hash.
6. Fallback while quota is pending: Colab or Kaggle for early experiments.
7. Always stop/delete VMs after a run; check billing daily during training weeks.

## Risks
- Package gaps on ARM64 (opencv, pyarrow, mujoco, mediapipe): handled by PyAV, polars, an ONNX hand-landmark model, and our own generator. New dependencies must be checked on the laptop before adoption.
- NPU speedup was measured only on a toy network; re-measure on the real models (M4 to M6). Unsupported operators fall back to CPU.
- Hand-landmark ONNX model: licence and accuracy not yet checked.
- Detector licences: prefer permissive (torchvision detectors, YOLOX); avoid AGPL.
- Sim-to-real needs a small real set filmed by you with a printed calibration board.
- Metric definitions must be checked against the literature before being called clinical.
