# Hand Sight spike round 4: confirm torch, mediapipe, mujoco, PyAV really work; test NPU on a heavier model.
# Run:  powershell -ExecutionPolicy Bypass -File .\spike\spike4.ps1
# Uses a disposable venv under %TEMP%. Takes about 10-15 minutes. Output: spike\spike4-report.md
$ErrorActionPreference = 'Continue'
$report = Join-Path $PSScriptRoot 'spike4-report.md'
$stack = Join-Path $PSScriptRoot 'probe_stack.py'
$npu = Join-Path $PSScriptRoot 'probe_npu.py'
$work = Join-Path $env:TEMP 'axonhand-spike4'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
$lines = New-Object System.Collections.ArrayList
function Add-Line($t) { [void]$lines.Add($t); Write-Host $t }
function Short($o) { (($o | Select-Object -Last 3) -join ' ') -replace '\s+', ' ' }

Add-Line '# Hand Sight spike report, round 4'
Add-Line ('Date: ' + (Get-Date -Format 's'))

$pyexe = 'python'
$resolved = (& py -3.12 -c "import sys;print(sys.executable)" 2>$null)
if ($LASTEXITCODE -eq 0 -and $resolved) { $pyexe = $resolved }
$pm = (& $pyexe -c "import platform,sys;print(platform.machine(), sys.version.split()[0])" 2>&1)
if ($LASTEXITCODE -ne 0) { Add-Line '- python NOT FOUND'; $lines | Set-Content -Encoding UTF8 $report; return }
Add-Line ('- python: ' + $pm)
$venv = Join-Path $work 'venv'
& $pyexe -m venv $venv 2>&1 | Out-Null
$vpy = Join-Path $venv 'Scripts\python.exe'
& $vpy -m pip install --quiet --upgrade pip 2>&1 | Out-Null

function Install($label, $pipArgs) {
    Write-Host ('installing ' + $label)
    $out = & $vpy -m pip install --quiet @pipArgs 2>&1
    if ($LASTEXITCODE -eq 0) { Add-Line ('- install ' + $label + ': OK') } else { Add-Line ('- install ' + $label + ': FAIL - ' + (Short $out)) }
}

Add-Line ''
Add-Line '## Installs'
Install 'core (numpy onnx onnxruntime onnxruntime-qnn av)' @('--only-binary=:all:', 'numpy', 'onnx', 'onnxruntime', 'onnxruntime-qnn', 'av')
Install 'torch (PyPI + PyTorch CPU index)' @('--only-binary=:all:', '--extra-index-url', 'https://download.pytorch.org/whl/cpu', 'torch')
Install 'mediapipe + pure deps' @('--only-binary=:all:', 'certifi', 'absl-py', 'attrs', 'flatbuffers', 'protobuf', 'sounddevice', 'matplotlib', 'mediapipe')
Install 'mujoco older than 4' @('--only-binary=:all:', 'mujoco<4')

Add-Line ''
Add-Line '## Do they actually work? (import plus a tiny task)'
if (Test-Path $stack) { $o = & $vpy $stack 2>$null; foreach ($l in $o) { Add-Line ([string]$l) } } else { Add-Line '- probe_stack.py missing' }

Add-Line ''
Add-Line '## NPU on a heavier model (fp32 and int8), with per-provider node counts'
if (Test-Path $npu) { $o = & $vpy $npu 2>$null; foreach ($l in $o) { $s = [string]$l; if ($s.StartsWith('- ')) { Add-Line $s } } } else { Add-Line '- probe_npu.py missing' }

Add-Line ''
Add-Line 'Done. See spike\spike4-report.md.'
$lines | Set-Content -Encoding UTF8 $report
Write-Host ('Report written to ' + $report)
