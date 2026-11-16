# Third-party notices

Hand Sight's own code is released under the MIT licence (see LICENSE). It includes, uses or
downloads the third-party material below, which keeps its own licence and is not covered by
Hand Sight's licence.

## Included in this repository

| Item | Where | Licence | Licence text |
| --- | --- | --- | --- |
| axe-core 4.10.2 (accessibility checker, copyright Deque Systems, Inc.), used only by the browser tests | `e2e/vendor/axe.min.js` | Mozilla Public License 2.0 | `e2e/vendor/LICENSE-axe-core.txt` |
| Space Grotesk (font, The Space Grotesk Project Authors) | `crates/ui/fonts/` | SIL Open Font License 1.1 | `crates/ui/fonts/LICENSE-SpaceGrotesk.txt` |
| IBM Plex Mono (font, IBM Corp.) | `crates/ui/fonts/` | SIL Open Font License 1.1 | `crates/ui/fonts/LICENSE-IBMPlexMono.txt` |

axe-core is included unmodified. Fonts are included as web font files (`.woff2`) and are
served with the app.

## Downloaded, not included

The hand-tracking models are not stored in this repository. `scripts/fetch_models.ps1` downloads
them into `models/` (which is excluded from version control), together with their licence files and
a record of their file hashes in `models/SOURCES.md`.

| Model | Source | Licence |
| --- | --- | --- |
| Palm detection (MediaPipe, ONNX conversion, `palm_detection_mediapipe_2023feb.onnx`) | OpenCV Zoo (github.com/opencv/opencv_zoo) | Apache License 2.0 |
| Hand landmarks (MediaPipe, ONNX conversion, `handpose_estimation_mediapipe_2023feb.onnx`) | OpenCV Zoo | Apache License 2.0 |

The models are the work of their original authors (Google's MediaPipe team and the OpenCV Zoo
contributors). The OpenCV Zoo licence file is the standard Apache 2.0 text with the copyright line
left blank; the evidence is in docs/HAND-MODEL-ASSESSMENT.md. Anyone using the models should read the
licence file that the script downloads. Results from the models are not clinical measurements.

## Software dependencies

These are installed by Cargo and pip when you build or run the project. They are not copied into
this repository.

- **Rust:** axum, tokio, tokio-stream, tower, tower-http, tracing, tracing-subscriber, serde,
  serde_json, toml, thiserror, sha2, rusqlite, futures-util, utoipa, nalgebra, ort (ONNX Runtime
  bindings), pyo3, leptos, leptos_router, wasm-bindgen, web-sys, gloo-net, console_error_panic_hook;
  for tests, proptest, tempfile and http-body-util. The exact versions are in `Cargo.lock`.
- **Python:** numpy, onnx, onnxruntime, av (PyAV), pillow; optional onnxruntime-qnn (Qualcomm NPU);
  for development, pytest, hypothesis, mypy, ruff, maturin; for browser tests, playwright.
- **Tools:** Trunk (builds the browser app) and LLVM.

These projects are published under their own licences, which are mostly MIT and Apache-2.0. To list
the exact licence of every Rust dependency, run `cargo install cargo-deny` and then
`cargo deny list`; for Python, `pip install pip-licenses` and then `pip-licenses`. Check these lists
before distributing a built copy of the software to other people.

## Not covered here

Videos you analyse belong to you and the people in them. They are personal data; see SECURITY.md.
