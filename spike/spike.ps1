# Hand Sight ARM64 spike: probes what installs natively on this machine.
# Run:  powershell -ExecutionPolicy Bypass -File .\spike\spike.ps1
# Installs only into a disposable venv under %TEMP%. Takes 10-20 minutes.
# Output: spike\spike-report.md
$ErrorActionPreference = 'Continue'
$report = Join-Path $PSScriptRoot 'spike-report.md'
$work = Join-Path $env:TEMP 'axonhand-spike'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
$lines = New-Object System.Collections.ArrayList
function Add-Line($t) { [void]$lines.Add($t); Write-Host $t }

Add-Line '# Hand Sight ARM64 spike report'
Add-Line ('Date: ' + (Get-Date -Format 's'))
Add-Line ''
Add-Line '## Machine'
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
Add-Line ('- OS: ' + $os.Caption + ' ' + $os.Version)
Add-Line ('- CPU: ' + $cpu.Name + ' (arch code ' + $cpu.Architecture + '; 12 = ARM64)')
Add-Line ('- RAM GB: ' + [math]::Round($os.TotalVisibleMemorySize / 1MB, 1))
Add-Line ('- PROCESSOR_ARCHITECTURE: ' + $env:PROCESSOR_ARCHITECTURE)
$gpus = (Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name }) -join '; '
Add-Line ('- GPU: ' + $gpus)
$npu = (Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { $_.FriendlyName -match 'NPU|Neural|Hexagon' } | ForEach-Object { $_.FriendlyName }) -join '; '
if (-not $npu) { $npu = 'none detected' }
Add-Line ('- NPU: ' + $npu)

Add-Line ''
Add-Line '## Toolchains'
$rv = (& rustc -vV 2>&1) -join ' | '
Add-Line ('- rustc: ' + $rv)
foreach ($d in @('C:\Program Files\LLVM\bin', 'C:\Program Files (x86)\LLVM\bin')) {
    if ((Test-Path (Join-Path $d 'clang.exe')) -and ($env:PATH -notlike ('*' + $d + '*'))) { $env:PATH = $d + ';' + $env:PATH }
}
# Prefer Python 3.12 (widest wheel support); fall back to whatever 'python' is.
$pyexe = 'python'
$resolved = (& py -3.12 -c "import sys;print(sys.executable)" 2>$null)
if ($LASTEXITCODE -eq 0 -and $resolved) { $pyexe = $resolved }
$py = $null
$pm = (& $pyexe -c "import platform,sys;print(platform.machine(), sys.version.split()[0])" 2>&1)
if ($LASTEXITCODE -eq 0) {
    $py = $true
    Add-Line ('- python: ' + $pm + '  (ARM64 = native, AMD64 = emulated x64)')
} else {
    Add-Line '- python: NOT FOUND (the Store shortcut does not count). Install Python 3.12 then open a NEW PowerShell window.'
}
$cl = Get-Command clang -ErrorAction SilentlyContinue
if ($cl) { Add-Line ('- clang: ' + $cl.Source) } else { Add-Line '- clang: NOT on PATH (open a NEW PowerShell window after installing LLVM)' }

if ($py) {
    Add-Line ''
    Add-Line '## Python wheel probes (binary wheels only, venv in TEMP)'
    $venv = Join-Path $work 'venv'
    & $pyexe -m venv $venv 2>&1 | Out-Null
    $vpy = Join-Path $venv 'Scripts\python.exe'
    & $vpy -m pip install --quiet --upgrade pip 2>&1 | Out-Null
    $pkgs = @('numpy', 'scipy', 'opencv-python-headless', 'onnxruntime', 'onnxruntime-qnn', 'mediapipe', 'mujoco', 'pyarrow', 'maturin', 'torch')
    foreach ($p in $pkgs) {
        Write-Host ('probing ' + $p)
        $out = & $vpy -m pip install --only-binary=:all: --quiet $p 2>&1
        if ($LASTEXITCODE -eq 0) {
            $ver = (& $vpy -m pip show $p 2>&1 | Select-String '^Version').ToString()
            Add-Line ('- ' + $p + ': OK ' + $ver)
        } else {
            $err = ($out | Select-Object -Last 2) -join ' '
            Add-Line ('- ' + $p + ': FAIL - ' + $err)
        }
    }
    $ort = & $vpy -c "import onnxruntime as o;print(o.get_available_providers())" 2>&1
    Add-Line ('- onnxruntime providers: ' + ($ort -join ' '))
    $t = & $vpy -c "import torch;print(torch.__version__, torch.zeros(1).device)" 2>&1
    Add-Line ('- torch import: ' + ($t -join ' '))
}

Add-Line ''
Add-Line '## Rust crate probes (cargo check)'
$crates = @('nalgebra', 'tract-onnx', 'ort', 'candle-core', 'pyo3')
foreach ($c in $crates) {
    Write-Host ('probing crate ' + $c)
    $dir = Join-Path $work ('rs-' + $c)
    & cargo init --name probe --vcs none $dir 2>&1 | Out-Null
    Push-Location $dir
    & cargo add $c 2>&1 | Out-Null
    $out = & cargo check 2>&1
    $code = $LASTEXITCODE
    Pop-Location
    if ($code -eq 0) { Add-Line ('- ' + $c + ': OK') }
    else { Add-Line ('- ' + $c + ': FAIL - ' + (($out | Select-Object -Last 3) -join ' ')) }
}

Add-Line ''
Add-Line 'Done. See spike\spike-report.md.'
$lines | Set-Content -Encoding UTF8 $report
Write-Host ('Report written to ' + $report)
