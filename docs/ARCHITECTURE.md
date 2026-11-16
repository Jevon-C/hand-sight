# Architecture

This describes what exists today. The longer plan and the phases are in `docs/PLAN.md`; the
reasons behind choices are in `docs/DECISIONS.md`.

## Parts

| Part | Crate or folder | Role | Status |
| --- | --- | --- | --- |
| Shared types | `crates/api` | Request, response and error types used by both server and browser app; also renders `docs/api/openapi.json` | Health, error, video, test, job and result types done |
| Server | `crates/server` | Axum program: settings, uploads, test registry, job queue and worker, result store, CSV export, serves the built app | Uploads, jobs, results, scores, landmarks, CSV; picks the real hand model when its files exist |
| Browser app | `crates/ui` | Leptos app compiled to WebAssembly: Analysis (with a slide-over set-up panel), Comparison and Library screens | Done, with a browser end-to-end test |
| Geometry core | `crates/geom`, `crates/geom-py` | Camera model, alignment, plane fit, board pose, smoothing; Python bindings as `axonhand.geometry` | Done |
| Python package | `python/` | Test plug-ins, metrics, tools | Plug-in tests (finger tapping, hand open-close, pronation-supination, postural tremor), metrics, synthetic data, evaluation and benchmark tools |
| Hand pipeline | `python/src/axonhand/hand/` | Palm detector, landmark model, tracking, video reading; ONNX Runtime on the NPU with CPU fallback | Built; runs on the NPU at about 107 frames per second on the development laptop |
| Hand model (Rust) | `crates/hand`, `crates/track` | A Rust port of the hand pipeline that matches Python (D12, docs/RUST-PORT.md) and the `axonhand-track` program the server can run | Done; identical to Python on the CPU |

## How a request flows

1. The browser app calls the server's JSON API under `/api`.
2. The server stores uploads and results as files named by content hash, and keeps an index in SQLite.
3. A job runs the hand model (the real two-stage model, or a stand-in colour tracker when its files are absent),
   then a Python test plug-in in a separate process. The server picks the real hand model when
   its files are in `models/` (setting `hand_model`). Jobs run one at a time from a SQLite queue.
4. The server checks the plug-in's output against the shared schema and stores it under a key made
   from the inputs, so the same inputs always return the same result.

## Rules that hold everywhere

- All request and response shapes live in `crates/api`. Nothing else defines wire formats.
- The server binds to `127.0.0.1` only unless `allow_remote` is set on purpose.
- Errors use one shape (`ApiError`); internal details go to the log, never to the client.
- No `unsafe` in our own code. `unwrap`, `expect` and `panic` are denied by lint.
- The API description in `docs/api/openapi.json` is generated and a test fails if it is stale.
  Refresh it with `cargo run -p axonhand-api --example write_openapi`.

## Running it

See the README. In short: build the app with Trunk (`crates/ui`), then run the server
(`scripts/dev.ps1` does both).
