# Hand Sight spike round 5: does mediapipe work without opencv-contrib?
# Run:  powershell -ExecutionPolicy Bypass -File .\spike\spike5.ps1   (about 2 minutes)
$ErrorActionPreference = 'Continue'
$report = Join-Path $PSScriptRoot 'spike5-report.md'
$probe = Join-Path $PSScriptRoot 'probe_mp.py'
$work = Join-Path $env:TEMP 'axonhand-spike5'
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Path $work | Out-Null
$lines = New-Object System.Collections.ArrayList
function Add-Line($t) { [void]$lines.Add($t); Write-Host $t }
function Short($o) { (($o | Select-Object -Last 3) -join ' ') -replace '\s+', ' ' }
Add-Line '# Hand Sight spike report, round 5 (mediapipe without opencv-contrib)'
Add-Line ('Date: ' + (Get-Date -Format 's'))
$pyexe = 'python'
$resolved = (& py -3.12 -c "import sys;print(sys.executable)" 2>$null)
if ($LASTEXITCODE -eq 0 -and $resolved) { $pyexe = $resolved }
& $pyexe -m venv (Join-Path $work 'venv') 2>&1 | Out-Null
$vpy = Join-Path $work 'venv\Scripts\python.exe'
& $vpy -m pip install --quiet --upgrade pip 2>&1 | Out-Null
$o = & $vpy -m pip install --quiet --only-binary=:all: numpy certifi absl-py attrs flatbuffers protobuf sounddevice matplotlib 2>&1
if ($LASTEXITCODE -eq 0) { Add-Line '- install pure dependencies: OK' } else { Add-Line ('- install pure dependencies: FAIL - ' + (Short $o)) }
$o = & $vpy -m pip install --quiet --only-binary=:all: --no-deps mediapipe 2>&1
if ($LASTEXITCODE -eq 0) { Add-Line '- install mediapipe --no-deps: OK' } else { Add-Line ('- install mediapipe --no-deps: FAIL - ' + (Short $o)) }
$o = & $vpy -m pip list --format=freeze 2>&1
Add-Line ('- mediapipe wheel installed: ' + (($o | Where-Object { $_ -match '^mediapipe' }) -join ' '))
if (Test-Path $probe) { $p = & $vpy $probe 2>$null; foreach ($l in $p) { Add-Line ([string]$l) } } else { Add-Line '- probe_mp.py missing' }
Add-Line ''
Add-Line 'Done. See spike\spike5-report.md.'
$lines | Set-Content -Encoding UTF8 $report
Write-Host ('Report written to ' + $report)
