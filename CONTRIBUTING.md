# Contributing

Hand Sight is a research prototype. Issues and pull requests are welcome.

## Before you open a pull request

1. Run the checks: `scripts\check.ps1` (Windows) or `scripts/check.sh`. They must pass; CI runs the
   same. For changes to the browser app also run `scripts\e2e.ps1`.
2. Keep to the quality contract in docs/QUALITY.md: no `unwrap`, `expect` or `panic` in library
   code, no `unsafe` in our own code, mypy strict, tests for new behaviour.
3. A decision that is not obvious from the code goes in docs/DECISIONS.md with its reason.
4. Never commit participant data, videos or model files (`data/` and `models/` are ignored).

## Adding a test

A test is a folder under `plugins/` with a manifest and a Python entry point. Start from
`plugins/_template` and follow docs/PLUGIN-AUTHORING.md. New measures must say how they are
defined and what is known about them in docs/METRICS.md, and are labelled illustrative until they
have been checked against hand-marked clips.

## Style

Rust: `cargo fmt`. Python: `ruff format`. Commit messages say what changed and why in plain words.
