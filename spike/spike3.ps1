# Hand Sight spike round 3: NPU via the device API, mediapipe/mujoco/torch retries, tract build.
# Run:  powershell -ExecutionPolicy Bypass -File .\spike\spike3.ps1
# Uses a disposable venv under %TEMP%. Takes about 10 minutes. Output: spike\spike3-report.md
$ErrorActionPreference = 'Continue'
$report = Join-Path $PSScriptRoot 'spike3-report.md'
$probe = Join-Path $PSScriptRoot 'probe_npu.py'
$work = Join-Path $env:TEMP 'axonhand-spike3'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
$lines = New-Object System.Collections.ArrayList
function Add-Line($t) { [void]$lines.Add($t); Write-Host $t }
function Short($o) { (($o | Select-Object -Last 3) -join ' ') -replace '\s+', ' ' }

Add-Line '# Hand Sight spike report, round 3'
Add-Line ('Date: ' + (Get-Date -Format 's'))

$llvm = 'C:\Program Files\LLVM\bin'
if (Test-Path (Join-Path $llvm 'clang.exe')) { $env:PATH = $llvm + ';' + $env:PATH; Add-Line ('- clang added to PATH for this run: ' + $llvm) }

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

function Probe-Pip($label, $pipArgs, $importCode) {
    Write-Host ('probing ' + $label)
    $out = & $vpy -m pip install --quiet @pipArgs 2>&1
    if ($LASTEXITCODE -ne 0) { Add-Line ('- ' + $label + ': INSTALL FAIL - ' + (Short $out)); return }
    if ($importCode) {
        $imp = & $vpy -c $importCode 2>&1
        if ($LASTEXITCODE -eq 0) { Add-Line ('- ' + $label + ': OK, ' + ($imp -join ' ')) }
        else { Add-Line ('- ' + $label + ': installed but FAIL - ' + (Short $imp)) }
    } else { Add-Line ('- ' + $label + ': OK') }
}

Add-Line ''
Add-Line '## NPU through the device API (fp32 and int8)'
Probe-Pip 'numpy onnx onnxruntime onnxruntime-qnn' @('--only-binary=:all:', 'numpy', 'onnx', 'onnxruntime', 'onnxruntime-qnn') $null
if (Test-Path $probe) { $o = & $vpy $probe 2>&1; foreach ($l in $o) { $s = [string]$l; if ($s -notmatch '^WARNING') { Add-Line $s } } }
else { Add-Line '- probe_npu.py missing next to this script' }

Add-Line ''
Add-Line '## mediapipe with its pure dependencies, without opencv-contrib'
Probe-Pip 'mediapipe deps' @('--only-binary=:all:', 'certifi', 'absl-py', 'attrs', 'flatbuffers', 'protobuf', 'numpy', 'sounddevice', 'matplotlib') $null
Probe-Pip 'mediapipe' @('--only-binary=:all:', '--no-deps', 'mediapipe') 'import mediapipe as mp; from mediapipe.tasks.python import vision; print("import ok", mp.__version__, hasattr(vision, "HandLandmarker"))'

Add-Line ''
Add-Line '## mujoco with its split core package'
Probe-Pip 'mujoco-core' @('--only-binary=:all:', 'mujoco-core') $null
Probe-Pip 'mujoco' @('--only-binary=:all:', 'mujoco') 'import mujoco; print("import ok", mujoco.__version__)'

Add-Line ''
Add-Line '## torch with PyPI plus the PyTorch CPU index'
Probe-Pip 'torch (extra index)' @('--only-binary=:all:', '--extra-index-url', 'https://download.pytorch.org/whl/cpu', 'torch') 'import torch; print("import ok", torch.__version__)'

Add-Line ''
Add-Line '## Rust: tract-onnx build with clang on PATH'
$dir = Join-Path $work 'rs-tract'
& cargo init --name probe --vcs none $dir 2>&1 | Out-Null
Push-Location $dir
& cargo add tract-onnx 2>&1 | Out-Null
$o = & cargo build 2>&1
$code = $LASTEXITCODE
Pop-Location
if ($code -eq 0) { Add-Line '- tract-onnx build: OK' }
else { Add-Line ('- tract-onnx build: FAIL - ' + ((($o | Where-Object { $_ -match 'error' } | Select-Object -First 3) -join ' ') -replace '\s+', ' ')) }

Add-Line ''
Add-Line 'Done. See spike\spike3-report.md.'
$lines | Set-Content -Encoding UTF8 $report
Write-Host ('Report written to ' + $report)
