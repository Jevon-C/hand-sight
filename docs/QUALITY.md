# Quality contract (draft: tighten as needed)

Rust: rustfmt; clippy `-D warnings`; no unwrap/expect/panic in library code; `unsafe` forbidden
unless justified in writing; `cargo audit` and `cargo deny` once dependencies grow.
Python: ruff; mypy strict; pytest; hypothesis for properties.
Geometry: property tests (project then unproject round-trip); fixed reprojection-error ceiling.
Metrics: synthetic motion with known parameters must be recovered within a stated percentage.
Reproducibility: lockfiles, seeds, data hashes, model cards; results keyed by
video hash + algorithm version + parameters + weights hash.
Performance: latency, model size and peak memory budgets in CI; numbers set after the first
baseline on the laptop (starting target: ~30 fps).
Platforms: tests run on Linux and Windows; ARM64 runs on the laptop (CI ARM runner unverified).
Run everything: `scripts/check.sh` or `scripts/check.ps1`.

## Platform rules (from the ARM64 spike)
- Python is pinned to 3.12 (`requires-python >=3.12,<3.13`). Dependencies must install as binary
  wheels on Windows ARM64; check on the laptop before adding one.
- No opencv, pyarrow or mujoco in laptop code paths; mediapipe only if the --no-deps install is proven. Video via PyAV; tables via polars.
- Rust crates needing a C compiler require clang on PATH (C:\Program Files\LLVM\bin).
- Accuracy checks accompany every quantised model: int8 versus fp32 output difference on real
  validation data, not random inputs.
- Benchmarks report CPU and NPU separately and the share of nodes the NPU executed.
