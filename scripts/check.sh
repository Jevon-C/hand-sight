#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Python first: the end-to-end Rust tests run the Python tools, so axonhand must be installed.
(
  cd python
  maturin develop --quiet
  ruff check . ../plugins
  ruff format --check . ../plugins
  mypy src tests
  for plugin in ../plugins/*/plugin.py; do mypy "$plugin"; done
  pytest -q
)
AXONHAND_PYTHON="$(command -v python)"
export AXONHAND_PYTHON
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
if rustup target list --installed | grep -q wasm32-unknown-unknown; then
  cargo check -p axonhand-ui --target wasm32-unknown-unknown
else
  echo 'note: wasm32-unknown-unknown target not installed; skipping the browser build check'
fi
