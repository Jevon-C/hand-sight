# Decisions log

## D1. Scalable means extensible plus reproducible (2026-11-12)
Plug-in tests and metrics; versioned, immutable results. Throughput scaling deferred.
See DESIGN-extensibility.md.

## D2. M0 runtime decision from the ARM64 spike (2026-11-12)
Machine: Surface laptop, Snapdragon X Plus X1P64100 (10-core Arm), Adreno GPU, Hexagon NPU,
15.6 GB RAM, Windows 11 on ARM64. Evidence: spike round 1 to 4 reports.

Decided:
- Edge runtime: Rust with `ort` (ONNX Runtime) and the Qualcomm QNN provider for the NPU.
  `tract-onnx` is the pure-Rust fallback; both build and link natively.
- NPU: registered through the device API (register the library, then add_provider_for_devices).
  On a synthetic 4-conv network (random weights, 384x384 input): CPU fp32 4.78 ms, NPU fp32
  1.22 ms; CPU int8 1.48 ms, NPU int8 0.49 ms. Profile shows QNN executing the graph.
  Caveats: toy network, random calibration; re-measure on the real models. The GPU path was
  not faster and failed for int8: not used.
- Python on the laptop: native ARM64 Python 3.12 (not 3.15: too new for wheels). Working:
  numpy, scipy, onnx, onnxruntime, onnxruntime-qnn, PyAV (encode and decode verified), pillow,
  polars, maturin, pyo3, and PyTorch 2.15 CPU (needs `--extra-index-url
  https://download.pytorch.org/whl/cpu`; forward, backward and ONNX export verified).
- Not installable natively via pip (checked against PyPI file lists, 2026-11-12): opencv and pyarrow
  (no win_arm64 wheel in any release); mujoco (the pure-Python `mujoco` needs `mujoco-core`, which
  has Windows x64 wheels only). mediapipe DOES publish a win_arm64 wheel (since 1.1.0), but it
  depends on opencv-contrib-python, which has none, so a normal install fails. Spike round 5:
  installed with --no-deps, `import mediapipe` still fails with "No module named cv2", so it
  needs opencv at import time and is NOT usable natively. Decision: mediapipe package is out;
  use the ONNX route for hand landmarks.
- Consequences: video decoding via PyAV; hand pose via an ONNX hand-landmark model (choice,
  licence and accuracy check pending); no MuJoCo dependency on the laptop (own parametric generator; MuJoCo or Blender may run on
  GCP Linux); heavy training on GCP, small experiments and ONNX export may run locally.
- Tooling: clang is at C:\Program Files\LLVM\bin and must be on PATH for crates that compile C.

## D3. Repo, licence, capture (2026-11-12, user)
- Repo name: axon_biomechanical_hand_evaluator (crate and package names stay `axonhand-*`).
- Licence: MIT. Copyright line reads "Jevon"; change if a full legal name is wanted.
- Capture: single camera, with a printed calibration board for metric scale (palm-length
  normalisation as fallback). Two-view capture is a possible later upgrade.

## D4. Hand-landmark model: provisional (2026-11-12)
See docs/HAND-MODEL-ASSESSMENT.md. Provisional choice: MediaPipe hand landmark weights
(Apache-2.0 per Google's model card) as an ONNX conversion run in ONNX Runtime, quantised
by us with real calibration data. Not final until the ONNX files are inspected, the palm
detector companion is confirmed, and accuracy is measured on our own data.

## D5. Working defaults for the open spec items (2026-11-12, user: "let's go")
See docs/SPEC-DEFAULTS.md (17 items). Revisit numbers after the first baseline.

## D6. M1 order of work (2026-11-12)
1. Camera model with Brown-Conrady distortion and reprojection error (done, 9 tests).
2. Rigid alignment (Kabsch) and plane fit (RANSAC). Done.
3. Pose from points (PnP) for the calibration board. Done (planar target).
4. DLT triangulation. Deferred, see D7.
5. Kalman and RTS smoother. Done.
6. PyO3 bindings and the reprojection-error ceiling test on synthetic scenes. Done.

## D7. Phase 0 geometry decisions (2026-11-12)
- Triangulation is deferred: the first release uses a single camera (D3), so nothing needs it.
  It returns if two-view capture is added.
- Pose from a flat target uses a direct linear homography with Hartley conditioning, then
  damped Gauss-Newton on reprojection error. The planar case is all the calibration board needs.
- RANSAC uses a small seeded generator (SplitMix64) instead of the `rand` crate, so results are
  repeatable and there is one fewer dependency.
- Smoother: constant-velocity Kalman filter with an RTS backward pass; NaN means missing.
- Python bindings: one wheel built by maturin from `crates/geom-py`, exposed as
  `axonhand.geometry`. PyO3 generates unsafe code, so that one crate uses `unsafe_code = "deny"`
  instead of `forbid`; all hand-written code in it is safe. Stability uses the abi3 interface,
  so one build works on Python 3.12 and 3.13.
- Minimum Rust version set to 1.82 (needed for `Option::is_none_or`).
- Python support range widened to 3.12 to 3.13 so the same code runs in this workspace and on the laptop.

## D8. Phase 1 decisions (2026-11-12)
- Browser app: Leptos 0.8, client-side rendered, built with Trunk and served by the server as static
  files. No server rendering, because the app runs locally and serves one user.
- Server: Axum 0.8 on Tokio. Settings come from defaults, an optional TOML file, then `AXONHAND_*`
  environment variables. Listening beyond this computer needs `allow_remote`.
- The API description is generated from the shared types with utoipa and checked in; a test keeps it current.
- The wasm target cannot be installed in the cloud workspace, so the browser build is checked natively
  there (`cargo check`) and for wasm on the laptop and in CI.
- Design tokens live in `crates/ui/style/tokens.css`, taken from the published design.

## D9. Phase 2 decisions (2026-11-12)
- Frames and video facts are read by Python workers using PyAV, run as separate processes
  (`python -m axonhand.tools.probe`). A crash or hang in the video library cannot take the server down,
  and the same tool works from the command line. A Rust decoder stays an option if speed demands it.
- A video's id is the SHA-256 of its bytes. A repeat upload returns the existing record and its original
  details; nothing is overwritten.
- Uploads stream to disk while being hashed. File type is decided by content, never by name. The
  uploaded name is display text only and never forms a path.
- The quality gate reports and never blocks. Checks that need the hand or board detectors are reported
  `not_checked` until phase 5, never as passes.
- SQLite via `rusqlite` (bundled), numbered migrations tracked in `user_version`; an index from a newer
  version is refused. Only the `videos` table exists so far; jobs, results and annotations come with the
  phases that need them.

## D10. Phase 3 decisions (2026-11-12)
- A test is a plug-in: a folder with `plugin.toml` and a script, run by the server as a separate process
  under a time limit and an output size limit. The server never imports plug-in code. Protocol in
  `docs/PLUGIN-PROTOCOL.md`.
- Manifests are parsed strictly (unknown keys are errors) and every problem is reported together.
- A plug-in that fails to load is still listed, as `broken` with its reasons, and never affects others.
  The folder name must equal the manifest id.
- Everything a plug-in returns is validated before use: format version, requested metrics only, each
  exactly once, finite values, and a reason for every missing value.
- Plug-in explanations of failure (`error.json`) are shown to the user, truncated and stripped of control
  characters. Anything on stdout or stderr stays in a log and is never sent to the browser.
- Planned tests (finger tapping, hand open and close, pronation and supination, postural tremor) are
  manifests with `status = "planned"`, so the Tests screen shows the roadmap from data, not code.
- The chosen test and metrics live in an app-wide `RunSelection` so later screens can start a run from it.

## D11. Phase 4 decisions (2026-11-12)
- A run has two steps: the hand model writes landmarks, then the test plug-in measures them. The
  model is behind one function type (`Extractor`), so the stand-in colour tracker is replaced by the
  real model in phase 5 without changing the queue, the API or the plug-ins.
- Results are keyed by everything that decides the numbers and are written once (see
  `docs/STORAGE.md`). Asking again returns the saved result; nothing is ever recalculated in place.
- Jobs run one at a time from a SQLite queue, so a restart loses nothing. Progress is available by
  polling (`GET /api/jobs/{id}`, used by the app) and as server-sent events (`/events`).
- Results made with the stand-in model are marked `illustrative` and shown with a banner and in the
  CSV header. A research prototype must never let development numbers look like measurements.
- Plots are drawn in the browser from the signals data by Rust code (no JavaScript chart library),
  with the geometry in a natively tested module.
- CSV export states test, version, model, settings and quality checks in `#` header lines, and guards
  text cells against spreadsheet formula injection.

## D12. Phase 5 decisions (2026-11-13)
- The first real hand pipeline is written in Python (`axonhand.hand`), not in a Rust runtime crate.
  Reason: ONNX Runtime with the NPU already works from Python on the laptop (spike), so the model
  can be judged on real video now. The Python pipeline becomes the reference: a later Rust runtime
  crate must reproduce its landmarks within a stated tolerance on fixed frames. The server only sees
  a command that turns a video into a landmark file, so swapping implementations changes no API.
- The pipeline is the MediaPipe design: a palm detector finds the hand once, a rotated square
  region is cut out for the landmark model, and later frames follow the hand from the previous
  landmarks, going back to the detector only when the landmark model is not confident (below 0.5).
  Our tracking box is measured in the hand's own turned frame, so tilting the hand does not grow it.
- All geometry (anchors, box decoding, regions, crop mapping) is plain numpy with exact tests;
  crop positions are checked against Pillow's own sampling to within 0.3 pixel. The tracker is tested
  with stand-in models, so its detect, follow and look-again logic is covered without a real hand.
- Phone rotation flags are applied when frames are read, so the model sees an upright picture.
- A result from the real model is still labelled illustrative. `Extractor.validated` becomes true
  only after the evaluation set (phase 5 exit check) supports it; the label is never removed by hand.
- Which model runs is a setting: `hand_model = auto` uses the real model when both ONNX files are
  in `models/`, otherwise the stand-in, and logs which. The model's hash covers both files and the
  pipeline version, so changing any of them gives new result keys.
- The hand model runs under a 20-minute limit; its output is kept next to the landmarks, and a
  failed job's scratch folder is kept so the log can be read.

## Open
- ONNX hand-landmark files: download, inspect tensors, confirm palm detector (needs permission).
- GCP project, budget alerts, GPU quota.

## D13: Evaluation by marked tap times (proposed, 2026-11-13)
Status: the method is built; the numeric limits are proposed and need the owner's confirmation.
Decision: the reference is tap times marked by a person at the moment the fingertips touch, compared with the
software's taps by one-to-one matching (docs/EVALUATION.md). Reason: it needs no extra hardware,
it measures the thing the tapping measures are built on, and it can be repeated by anyone.
Consequence: amplitude measures are not validated by this method. The "Illustrative" label stays
until a report on at least 10 real clips meets the limits.
Revised 2026-11-13 after the first real clip (17 taps): marking the widest opening was not
repeatable on slow openings (6 frames apart on one tap), so the reference is now the moment of
contact, which the software finds from the opening signal (`contact_frames`). The match
tolerance changed from a quarter to half of the typical gap, and the timing limit from 50 ms to
70 ms (two frames at 30 fps), tentatively. These were changed after seeing a result, for the
reasons above; they were not tuned to pass.
First real clip, second finding (2026-11-13): marking contacts exposed a fault in the software, not
in the reference. A hand held open and slightly wobbling for 0.8 s before the tapping began was
counted as two taps, because a tap only had to be 0.15 palm lengths high. A tap must now also
close again (by half of that), and the finger-tapping test is version 0.2.0. Result: 15 taps and
15 touches found against 15 marked.

## D14: Literature check, smoothing, speed and quantisation tools (2026-11-13)
Literature check (docs/METRICS.md): our definitions agree with three published video studies on
the signal, the tap rate and the amplitude decrement, and differ on amplitude (widest opening, not
peak minus trough), on the hesitation and arrest thresholds (ours 1.5 and 3 times the median gap;
the one precedent found is 2) and on smoothing. We keep ours and say so; none of these is
validated. The minimum frame rate in the finger-tapping test is lowered from 50 to 30 (60
preferred), since the papers used 25 to 30 and the evaluation's timing limit is set by two frames
at 30.
Smoothing: kept the 3-point average; the Kalman smoother is better on synthetic data when told
the true noise, which is not known in practice (docs/METRICS.md).
Tools added: `axonhand.compare_markers` (spread between two people), `axonhand.tools.benchmark`
(speed by stage and device), `axonhand.tools.quantize` (int8 landmark model and its measured
error, with proposed limits of 0.02 palm lengths median and 0.05 at the 95th percentile, same tap
count and contact times within one frame). The quantised model is used only if it meets them on
the NPU and the speed gain is worth having; the float model on the NPU already runs the whole
pipeline at over 100 frames per second.


## D15: Speed and quantisation results; float model stays (2026-11-13)
Measured on the Snapdragon X Plus laptop, one 386-frame clip (`data/bench/benchmark.md`):
CPU only 39.3 ms per frame (25 fps); NPU for both models 9.3 ms per frame (107 fps). Per model
the NPU takes 0.80 ms (palm) and 0.53 ms (landmarks) against 10.2 ms and 5.0 ms on the CPU.
Decoding the video (6.6 ms per frame) is now the largest single cost. Peak memory 528 MB.
Int8 landmark model (`data/quant`, `data/quant16`): not adopted.
- The QNN provider refused to load either quantised model (convolution layout error), so there
  is no NPU gain to weigh.
- 8-bit weights and activations on the CPU: 124 ms per call against 21 ms for float, and
  landmark error (median 0.0225, 95th percentile 0.0683 palm lengths) outside both limits.
- 16-bit activations: landmark error within limits (0.0139, 0.0231) but 14 taps found against 15
  and a contact time 33 ms off, so the tap-count limit fails.
The float model on the NPU already exceeds real time several times over, so the decision costs
nothing. Making a QNN-compatible quantised model (matching layout, vendor tooling) is left as
possible future work. One clip only: these are timings, not accuracy evidence.

## D16: Rust hand crate, checked against Python (2026-11-13)
The hand pipeline now exists in Rust (`crates/hand`, docs/RUST-PORT.md). Python stays the
reference until parity is shown on a real clip; the Rust crate is checked against numbers the
Python code produced, so the two cannot drift silently. We chose to reproduce Pillow's resize
and crop byte for byte instead of accepting a tolerance, since a one-level difference in the crop
can move landmarks and make "agreement" hard to interpret. ONNX Runtime is loaded from a shared
library at run time, so no binary is downloaded at build time and the same crate builds on the
Arm64 laptop and the Linux sandbox. NPU placement from Rust is left for the laptop.
Addendum (2026-11-13): on the laptop the Rust tracker on the NPU runs at about 2 to 3 ms per
frame against about 20 ms on the CPU, with landmarks within the proposed limits of the CPU ones
(median 0.0042, 95th percentile 0.0166 palm lengths) and the same taps and contact times on 120
frames. We keep `Device::Auto` as the default (NPU, falling back to CPU with the reason recorded).
Recorded in docs/RUST-PORT.md.

## D17: Three more hand tests, checked only against synthetic hands (2026-11-13)
`hand_open_close`, `pronation_supination` and `postural_tremor` are now available plug-ins (version
0.1.0). They share one design: a measured signal from the landmarks, a clear definition of the
event or band, the same rule for hesitations and arrests as finger tapping, and quality checks
that say when the numbers are not to be trusted. Their only validation is synthetic truth, so
they remain "Illustrative" and docs/METRICS.md says what is not known (notably: no angle in
pronation/supination, and an assumed flat tracker noise in tremor). The tremor plug-in refuses
videos under 5 seconds or at a frame rate that cannot see the band, instead of reporting a number.

## D18: Overlay saved with the result; Compare refuses different tests (2026-11-13)
The landmarks for drawing over the video are saved inside the result folder (hashed like the
other files) instead of being recomputed, so a result always shows exactly the track its numbers
came from. Results made without them (the stand-in model, or before this change) simply show the
video with no overlay. The Compare screen only sets results of the same test side by side, since
numbers of different tests do not mean the same thing, and it states plainly when the test
version, hand model or settings differ, because those can explain a difference that is not about
the patient. It shows the difference and its relative size but no verdict: no threshold of
"change that matters" has been validated.

## D19: Browser test with Playwright and Edge, outside the main check (2026-11-13)
The browser test drives the real server and the built web app with Playwright (Python, so it
shares the project's tooling) in Microsoft Edge, because Edge is on every Windows machine and
plays the H.264 test videos that Playwright's own Chromium cannot. It uses the stand-in hand
model and synthetic videos so it is repeatable and needs no recordings or model files. It is a
separate script (`scripts/e2e.ps1`) rather than part of `check.ps1` because it needs Trunk, a
browser and a minute or two; the fast checks stay fast. It also fails on any console error, so
a broken web-assembly build shows up here and not only in a person's browser.

## D20: Clinician scores are per-rater facts beside the result (2026-11-13)
A score is stored in the index with the rater's name, not in the result folder, because results
are write-once and a score is a separate, correctable fact. One row for each result and rater lets
several clinicians score the same video, which the validation needs (rater agreement as well as
agreement with the measures). The rater name is required so every score can be attributed. The
scale is the MDS-UPDRS 0 to 4 for every test, since all the planned tests are part III items. The
app does not hide the measures from the rater; docs/STORAGE.md says what that means for a study.

## D21: The Rust tracker is a separate program fed by a pipe (2026-11-13)
The server can run the Rust hand tracker in place of the Python one. It is a separate program,
like the plug-ins, so a fault in it cannot take the server down and it can be killed on timeout.
Python keeps decoding video: PyAV already handles rotation, odd codecs and variable frame rates,
and rewriting that would add risk and no measured benefit. The decoder writes a one-line header
and raw RGB frames to the tracker's standard input; exit code 2 from either program means "this
video cannot be used" and carries a plain message, any other failure is a fault. Results get a
different model hash (`-rust` suffix) so the two trackers' results never mix. The automatic choice
stays the Python tracker until the Rust one has been compared on more than one real clip
(`scripts/try_rust_tracker.ps1`); then it can be flipped.

## D22: The CPU is the default device; the NPU is a choice, and a separate result (2026-11-13)
Measured on a second real clip (367 frames): the Rust tracker on the CPU is identical to the Python
tracker on the CPU in every frame (same taps, 0.0000 error), while the same Rust tracker on the
NPU differs from the CPU by up to half a palm length for a fingertip in fast frames (median
0.0075, 95th percentile 0.066 against a limit of 0.05), with the taps still agreeing within one
frame and 0.56% on rate. The NPU is about 3.5 times faster (3.0 s against 10.5 s of tracking).
Until then the app used `auto`, which on the laptop meant the NPU, and the result key did not say
which device had produced a result, so one video could give slightly different measurements on
different computers. Now a request names the device (`device`: `cpu` by default, or `npu`); it is
part of the model identity (a different hash, so a different result key) and is recorded and
shown with each result. The CPU is the default because it is exact, repeatable on any computer and
the same for both trackers; the NPU is offered on the Run page for speed. Asking for the NPU on a
computer without one fails with a plain message, never a silent fallback to the CPU. The Compare
screen explains a CPU against NPU comparison as the NPU's lower precision. A stand-in does not
depend on the device. Results saved before this change have no device recorded and keep their keys.
Loading the NPU plug-in changes what a CPU run produces (it moved the models onto the NPU), so a
CPU run never loads it (docs/RUST-PORT.md).

## D23: Automatic hand model order, and the landmark comparison (2026-11-13)
`AXONHAND_HAND_MODEL=auto` (the default) now chooses the Rust tracker if it can run (the model
files, the `axonhand-track` program next to the server or on `PATH`, and ONNX Runtime found through
Python), otherwise the Python pipeline with the same models, otherwise the development stand-in.
Every step down is logged with its reason. The Rust tracker is first because on the CPU it is
identical to the Python pipeline on two real clips and faster (D22), so nothing is given up;
`mediapipe` still forces Python and `mediapipe_rust` demands Rust. `scripts/dev.ps1` builds the
tracker so the server finds it. The device is separate (D22): the CPU unless the Run page asks for
the NPU.

The Compare screen shows how far apart two results of the same video put the hand landmarks
(median, 95th percentile and largest error in palm lengths, the number of frames over the limit,
and whether the proposed limits are met), computed in the browser from the two saved landmark
files. It is the same measure as `axonhand.tools.compare_landmarks`, so a CPU against NPU
comparison in the app gives the numbers the script gave. The saved landmarks are rounded to a
tenth of a pixel, so differences below about 0.001 palm lengths are rounding.

