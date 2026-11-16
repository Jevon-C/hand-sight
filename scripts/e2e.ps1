# Browser end-to-end test: builds the web app and the server, then drives them in a real browser.
#
# Run from the repo root, in the activated venv window:
#   powershell -ExecutionPolicy Bypass -File .\scripts\e2e.ps1
# Needs: Rust, the wasm32-unknown-unknown target, Trunk, and Microsoft Edge (see README).
# Set AXONHAND_E2E_BROWSER to use another Playwright channel, or chromium for Playwright's own.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
function Run($c) { Write-Host ">> $c"; Invoke-Expression $c; if ($LASTEXITCODE -ne 0) { throw "failed: $c" } }

Run 'python -m pip install --quiet "playwright>=1.45" pytest'
if ($env:AXONHAND_E2E_BROWSER -eq 'chromium') { Run 'python -m playwright install chromium' }
Push-Location 'crates\ui'
try { Run 'trunk build' } finally { Pop-Location }
Run 'cargo build -p axonhand-server'
Run 'python -m pytest e2e -v'
