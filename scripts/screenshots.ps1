# Screenshots of every screen at phone, tablet and desktop widths, for design review.
#
# Run from the repo root, in the activated venv window:
#   powershell -ExecutionPolicy Bypass -File .\scripts\screenshots.ps1
# Builds the web app and the server, then drives them in a real browser (see e2e\capture_screens.py).
# The pictures and an accessibility report are written to target\screenshots, and zipped to
# target\screenshots.zip so they can be shared in one go.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
function Run($c) { Write-Host ">> $c"; Invoke-Expression $c; if ($LASTEXITCODE -ne 0) { throw "failed: $c" } }

Run 'python -m pip install --quiet "playwright>=1.45" pytest'
if ($env:AXONHAND_E2E_BROWSER -eq 'chromium') { Run 'python -m playwright install chromium' }
Push-Location 'crates\ui'
try { Run 'trunk build' } finally { Pop-Location }
Run 'cargo build -p axonhand-server'
# The server keeps its data in target\screenshots-data; start each run from nothing.
if (Test-Path 'target\screenshots-data') { Remove-Item -Recurse -Force 'target\screenshots-data' }
Run 'python e2e\capture_screens.py'
if (Test-Path 'target\screenshots.zip') { Remove-Item -Force 'target\screenshots.zip' }
Compress-Archive -Path 'target\screenshots\*' -DestinationPath 'target\screenshots.zip'
Write-Host "Zipped: $((Resolve-Path 'target\screenshots.zip').Path)"
