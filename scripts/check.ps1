$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
function Run($c) { Write-Host ">> $c"; Invoke-Expression $c; if ($LASTEXITCODE -ne 0) { throw "failed: $c" } }
# Python first: the end-to-end Rust tests run the Python tools, so axonhand must be installed.
Set-Location python
Run 'maturin develop --quiet'
Run 'ruff check . ../plugins'
Run 'ruff format --check . ../plugins'
Run 'mypy src tests'
foreach ($plugin in Get-ChildItem -Path ..\plugins -Filter plugin.py -Recurse) {
    Run "mypy `"$($plugin.FullName)`""
}
Run 'pytest -q'
Set-Location ..
$env:AXONHAND_PYTHON = (Get-Command python).Source
Run 'cargo fmt --all -- --check'
Run 'cargo clippy --all-targets -- -D warnings'
Run 'cargo test --all'
if ((rustup target list --installed) -contains 'wasm32-unknown-unknown') {
    Run 'cargo check -p axonhand-ui --target wasm32-unknown-unknown'
} else {
    Write-Host 'note: wasm32-unknown-unknown target not installed; skipping the browser build check'
}
