# Build the browser app and start the server.
#
# Run from the repo root:  powershell -ExecutionPolicy Bypass -File .\scripts\dev.ps1
# Needs: Rust, the wasm32-unknown-unknown target, and Trunk (see README). Then open
# http://127.0.0.1:8080 in a browser. Press Ctrl+C to stop.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

Push-Location 'crates\ui'
try {
    & trunk build
    if ($LASTEXITCODE -ne 0) { throw 'trunk build failed' }
} finally {
    Pop-Location
}
# The server runs the Python tools with the project's virtual environment if there is one.
$venvPython = Join-Path (Get-Location) '.venv\Scripts\python.exe'
if (Test-Path $venvPython) { $env:AXONHAND_PYTHON = $venvPython }
# The server uses the Rust hand tracker when it finds the program next to itself, so build it too.
& cargo build -p axonhand-server -p axonhand-track
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
& cargo run -p axonhand-server
if ($LASTEXITCODE -ne 0) { throw 'server exited with an error' }
