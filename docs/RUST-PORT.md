# Rust port of the hand pipeline

The crate `crates/hand` (`axonhand-hand`) does what `python/src/axonhand/hand/` does: find the
palm, cut out the hand, find the 21 landmarks, and follow the hand from frame to frame. It takes
frames as `RgbImage`; decoding video stays outside it.

| Rust | Python | What it is |
|---|---|---|
| `rois.rs` | `rois.py` | anchors, palm decoding, rectangles, crop matrix; no model in it |
| `image.rs` | Pillow | resize and affine crop, written to give Pillow's exact bytes |
| `runtime.rs` | `runtime.py` | ONNX Runtime library, opening a model, running it on one input |
| `tracker.rs` | `tracker.py` | detect once, then follow; look again when the hand is lost |

## How we know the two agree

`python -m axonhand.tools.parity_fixture crates/hand/tests/fixtures/parity.json --models models`
runs the Python code on fixed inputs and saves the answers. `crates/hand/tests/parity.rs` checks
the Rust code against them:

- anchors, letterbox sizes, rectangles, crop matrices and back-projection agree to 1e-9;
- palm decoding (with overlap removal) gives the same palms;
- **resize and crop give the same bytes as Pillow** (difference 0). Pillow's resize uses integer
  weights with 22 fractional bits, and its affine crop truncates instead of rounding; the Rust
  code does the same, because matching to within one level was not enough to be sure of identical
  landmarks;
- both ONNX models give the same outputs on a fixed picture (sampled every 41st value, within
  1e-3), run on the CPU.

The last two need the ONNX Runtime library and the models, so they run only when `ORT_DYLIB_PATH`
and `AXONHAND_MODELS` are set; otherwise they print "skipped". The runtime is loaded from a shared
library at run time (`ort` feature `load-dynamic`), so building needs no download. Use the one
inside the Python package: `onnxruntime/capi/libonnxruntime.so.*` (Linux) or
`onnxruntime/capi/onnxruntime.dll` (Windows).

### End to end on a real clip

```
python -m axonhand.tools.parity_fixture data/parity_clip --clip data/try/tapping-real.mp4 --models models --frames 120
```

writes the first frames and Python's landmarks; `clip_matches_python` runs the whole Rust tracker
over them and fails if any landmark differs by 0.05 pixel or more, or if the two disagree on
whether a hand is present. Result on the laptop (Arm64 Windows, 2026-11-13, CPU, 120 frames of the real tapping clip, a hand
present throughout): largest landmark difference 0.000000 pixels, and the two agree on every frame.
Both sides run the same ONNX Runtime on the same bytes, so identical output is what exactness of
the image operations predicts; it shows the Rust crate is a faithful port, not that either is
accurate.

## NPU from Rust

`Runtime::register_npu_library(name, library)` registers the vendor's plug-in once (a second call
does nothing), then `Model::open_on(runtime, path, Device::Auto | Cpu | Npu)` places the model on
the first device that is an NPU. `Auto` falls back to the CPU and `Model::note()` says why;
`Npu` fails instead. `Model::placement()` reports where it really runs.

The example `track_clip` runs the tracker over exported frames and prints the speed and the
difference from Python. On the laptop:

```
$qnn = python -c "import onnxruntime_qnn as q; print(q.get_ep_name()); print(q.get_library_path())"
cargo run --release -p axonhand-hand --example track_clip -- --library <onnxruntime.dll> --models models --clip data\parity_clip --device npu --npu-name <name> --npu-library <path>
```

Run it with `--device cpu` too, for the comparison. NPU and CPU landmarks are not expected to be
identical (the NPU computes with different rounding); the difference is what is reported.

### Measured on the laptop (2026-11-13)

First 120 frames of the real tapping clip (hand present in all), Rust tracker, release build:

| Models on | Time per frame | Frames per second | Landmarks against Python (CPU) |
|---|---|---|---|
| CPU | 19.7 to 23.0 ms | 43 to 51 | identical (0 px) |
| NPU | 1.94 to 2.89 ms | 346 to 517 | median 0.0042, 95th percentile 0.0166, largest 0.0692 palm lengths (ring fingertip, frame 32) |

The NPU result is inside the proposed limits (median 0.02, 95th percentile 0.05; the largest value
is above 0.05 but the limit is on the 95th percentile). On the 4 taps in these frames the NPU
finds the same 4 and the same contact times (0.0 ms difference); the tap rate differs by +1.52%,
about one frame of shift in the span between first and last peak (a 4-tap, 3-interval window, so
one frame is a large fraction). This is a short window: the full clip (15 taps) has not been run
through Rust on the NPU, and one clip is not an accuracy result. The speed includes cropping and
resizing but not video decoding. The timing is for frames already decoded, so it is not
comparable with the 9.3 ms per frame of the Python pipeline, which includes decoding.

## The tracker program and the server (2026-11-13)

`crates/track` builds `axonhand-track`, a command that reads decoded frames on standard input and
writes the landmark track (`.npz`, readable by NumPy) and the overlay (`.json`) the server shows.
Python still decodes the video (PyAV): `python -m axonhand.tools.decode_frames VIDEO` writes a
one-line header (`AXONFRAMES 1 W H FPS`) and then raw RGB frames, and the server connects the
two programs. The server uses this when `AXONHAND_HAND_MODEL=mediapipe_rust` (binary found via
`AXONHAND_TRACK`, default `axonhand-track`). Results from it carry a different model hash from
the Python tracker's, so the two never share a result. See DECISIONS D21 and README.

`scripts/try_rust_tracker.ps1` runs both trackers on one video and compares them. In the
sandbox the pipe was tested with the real models for the no-hand and not-a-video cases; the
hand-found case is checked on the laptop with that script.

### Finding: loading the NPU plug-in changes a CPU run (2026-11-13)

On a second real clip (367 frames) the tracker program differed from Python by up to half a palm
length (95th percentile 0.066, limit 0.05) although the in-process Rust tracker was exact on the
same frames (0.000000 px over 40). The cause was that the program registered the NPU plug-in
library even when told to use the CPU; the numbers a CPU run produced then differed. With the
plug-in left out for `--device cpu` the program is identical to Python in all 367 frames (error
0.0000, same taps). `axonhand-track` now loads the plug-in only for `auto` and `npu`. A run that
does use the NPU gives different numbers from the CPU by the NPU's own numerics, which is a
separate question from whether the port is faithful, and is measured with
`scripts/try_rust_tracker.ps1 -Device auto` against a CPU run.

### Result on two real clips (laptop, 2026-11-13)

CPU: identical to Python in every frame of both clips, including the taps. NPU on the second
clip: median error 0.0075 palm lengths, 95th percentile 0.066 (proposed limit 0.05), 84 of 367
frames with some landmark off by more than 0.05, worst 0.50 in a burst of fast frames; taps 8
against 8, contact times within one frame, rate -0.56%. That is the NPU's precision, not the
port (DECISIONS D22). NPU tracking 3.0 s against 10.5 s on the CPU for 367 frames.

## Not done yet

- Nothing blocking. The automatic model choice now prefers the Rust tracker (DECISIONS D23) and
  the Compare screen shows the landmark difference between a CPU and an NPU result of one video.
  Decoding stays in Python (PyAV).
- Decoding in Rust (not planned; PyAV stays).
