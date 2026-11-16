# Hand Sight spike round 2: video decoding, hand-pose prerequisites, ONNX Runtime + NPU.
# Run:  powershell -ExecutionPolicy Bypass -File .\spike\spike2.ps1
# Uses a disposable venv under %TEMP%. Takes 10-20 minutes. Output: spike\spike2-report.md
$ErrorActionPreference = 'Continue'
$report = Join-Path $PSScriptRoot 'spike2-report.md'
$probe = Join-Path $PSScriptRoot 'probe_ort.py'
$work = Join-Path $env:TEMP 'axonhand-spike2'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
$lines = New-Object System.Collections.ArrayList
function Add-Line($t) { [void]$lines.Add($t); Write-Host $t }
function Short($o) { (($o | Select-Object -Last 3) -join ' ') -replace '\s+', ' ' }

Add-Line '# Hand Sight spike report, round 2'
Add-Line ('Date: ' + (Get-Date -Format 's'))

# --- where is clang / ffmpeg? ---
Add-Line ''
Add-Line '## Tools on this machine'
$found = @()
foreach ($root in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, "$env:LOCALAPPDATA\Programs")) {
    if ($root -and (Test-Path $root)) {
        $found += Get-ChildItem -Path $root -Filter clang.exe -Recurse -ErrorAction SilentlyContinue -Depth 4 | ForEach-Object { $_.FullName }
    }
}
if ($found.Count -gt 0) { Add-Line ('- clang.exe found at: ' + ($found -join '; ')) } else { Add-Line '- clang.exe: not found under Program Files (LLVM may not be installed)' }
$ff = Get-Command ffmpeg -ErrorAction SilentlyContinue
if ($ff) { Add-Line ('- ffmpeg: ' + $ff.Source) } else { Add-Line '- ffmpeg: not on PATH' }
$wg = (& winget list --id LLVM.LLVM 2>&1) -join ' '
Add-Line ('- winget LLVM: ' + ($wg -replace '\s+', ' '))

# --- python venv ---
$pyexe = 'python'
$resolved = (& py -3.12 -c "import sys;print(sys.executable)" 2>$null)
if ($LASTEXITCODE -eq 0 -and $resolved) { $pyexe = $resolved }
$pm = (& $pyexe -c "import platform,sys;print(platform.machine(), sys.version.split()[0])" 2>&1)
if ($LASTEXITCODE -ne 0) {
    Add-Line '- python: NOT FOUND. Install Python 3.12 and open a NEW PowerShell window.'
    $lines | Set-Content -Encoding UTF8 $report
    return
}
Add-Line ('- python: ' + $pm)
$venv = Join-Path $work 'venv'
& $pyexe -m venv $venv 2>&1 | Out-Null
$vpy = Join-Path $venv 'Scripts\python.exe'
& $vpy -m pip install --quiet --upgrade pip 2>&1 | Out-Null

function Probe-Pip($label, $pipArgs, $importName) {
    Write-Host ('probing ' + $label)
    $out = & $vpy -m pip install --quiet @pipArgs 2>&1
    if ($LASTEXITCODE -ne 0) { Add-Line ('- ' + $label + ': INSTALL FAIL - ' + (Short $out)); return }
    if ($importName) {
        $imp = & $vpy -c ("import " + $importName + ";print('import ok', getattr(" + $importName + ", '__version__', ''))") 2>&1
        if ($LASTEXITCODE -eq 0) { Add-Line ('- ' + $label + ': OK, ' + ($imp -join ' ')) }
        else { Add-Line ('- ' + $label + ': installed but import FAIL - ' + (Short $imp)) }
    } else { Add-Line ('- ' + $label + ': OK') }
}

Add-Line ''
Add-Line '## Video decoding and imaging'
Probe-Pip 'av (PyAV)' @('--only-binary=:all:', 'av') 'av'
Probe-Pip 'imageio-ffmpeg' @('--only-binary=:all:', 'imageio-ffmpeg') 'imageio_ffmpeg'
Probe-Pip 'opencv-python (full)' @('--only-binary=:all:', 'opencv-python') 'cv2'
Probe-Pip 'pillow' @('--only-binary=:all:', 'pillow') 'PIL'
Probe-Pip 'polars (pyarrow alternative)' @('--only-binary=:all:', 'polars') 'polars'

Add-Line ''
Add-Line '## ONNX and NPU'
Probe-Pip 'onnx' @('--only-binary=:all:', 'onnx') 'onnx'
Probe-Pip 'onnxruntime' @('--only-binary=:all:', 'onnxruntime') 'onnxruntime'
Probe-Pip 'onnxruntime-qnn' @('--only-binary=:all:', 'onnxruntime-qnn') $null
if (Test-Path $probe) {
    Write-Host 'running ONNX Runtime probe'
    $o = & $vpy $probe 2>&1
    foreach ($l in $o) { Add-Line ([string]$l) }
} else { Add-Line '- probe_ort.py missing next to this script' }

Add-Line ''
Add-Line '## Hand pose and simulation packages (no-deps installs, then import)'
Probe-Pip 'mediapipe --no-deps' @('--only-binary=:all:', '--no-deps', 'mediapipe') 'mediapipe'
Probe-Pip 'mujoco --no-deps' @('--only-binary=:all:', '--no-deps', 'mujoco') 'mujoco'

Add-Line ''
Add-Line '## PyTorch from the official CPU index'
Probe-Pip 'torch (pytorch cpu index)' @('--only-binary=:all:', '--index-url', 'https://download.pytorch.org/whl/cpu', 'torch') 'torch'

Add-Line ''
Add-Line '## Rust: actual build and link (not just check)'
foreach ($c in @('ort', 'tract-onnx')) {
    Write-Host ('building crate ' + $c)
    $dir = Join-Path $work ('rs-' + $c)
    & cargo init --name probe --vcs none $dir 2>&1 | Out-Null
    Push-Location $dir
    & cargo add $c 2>&1 | Out-Null
    $o = & cargo build 2>&1
    $code = $LASTEXITCODE
    Pop-Location
    if ($code -eq 0) { Add-Line ('- ' + $c + ' build: OK') } else { Add-Line ('- ' + $c + ' build: FAIL - ' + (Short $o)) }
}

Add-Line ''
Add-Line 'Done. See spike\spike2-report.md.'
$lines | Set-Content -Encoding UTF8 $report
Write-Host ('Report written to ' + $report)
