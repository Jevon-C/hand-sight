# Download the two float ONNX hand models and their licence files, record hashes, and
# inspect them on the CPU and NPU.
#
# Run from the repo root:  powershell -ExecutionPolicy Bypass -File .\scripts\fetch_models.ps1
# Needs: the Python venv from the README activated (so `python -m axonhand.tools.inspect_model`
# works). Output: models\ (not committed), models\SOURCES.md, models\inspect-report.md.
param([switch]$SkipDownload)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$base = 'https://github.com/opencv/opencv_zoo/raw/main/models'
$items = @(
    @{ Folder = 'palm_detection_mediapipe'; File = 'palm_detection_mediapipe_2023feb.onnx' },
    @{ Folder = 'handpose_estimation_mediapipe'; File = 'handpose_estimation_mediapipe_2023feb.onnx' }
)
New-Item -ItemType Directory -Force -Path 'models' | Out-Null
$sources = New-Object System.Collections.ArrayList
[void]$sources.Add('# Model sources')
[void]$sources.Add('')
[void]$sources.Add('Hashes are recorded at download time. Compare them if a file is ever replaced.')
[void]$sources.Add('')

foreach ($item in $items) {
    if ($SkipDownload) { continue }
    $dest = Join-Path 'models' $item.File
    $url = "$base/$($item.Folder)/$($item.File)"
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
    $size = (Get-Item $dest).Length
    $head = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($dest)[0..40])
    if ($size -lt 500000 -or $head -like 'version https://git-lfs*') {
        throw "$($item.File) looks like a Git LFS pointer or a failed download ($size bytes)."
    }
    $hash = (Get-FileHash -Algorithm SHA256 $dest).Hash.ToLower()
    $licenceUrl = "$base/$($item.Folder)/LICENSE"
    $licenceDest = Join-Path 'models' "$($item.Folder).LICENSE.txt"
    Invoke-WebRequest -Uri $licenceUrl -OutFile $licenceDest -UseBasicParsing
    $licenceFirst = (Get-Content $licenceDest -TotalCount 3) -join ' '
    [void]$sources.Add("## $($item.File)")
    [void]$sources.Add("- Source: $url")
    [void]$sources.Add("- Size: $size bytes")
    [void]$sources.Add("- SHA-256: $hash")
    [void]$sources.Add("- Licence file: $licenceDest ($licenceFirst)")
    [void]$sources.Add("- Downloaded: $(Get-Date -Format 's')")
    [void]$sources.Add('')
}
if (-not $SkipDownload) { $sources | Set-Content -Encoding UTF8 'models\SOURCES.md' }

$report = New-Object System.Collections.ArrayList
foreach ($item in $items) {
    # Native tools write warnings to stderr; that must not stop the script.
    $ErrorActionPreference = 'Continue'
    $out = & python -m axonhand.tools.inspect_model (Join-Path 'models' $item.File) --runs 50 2>&1 |
        ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "inspect_model failed for $($item.File): $($out -join ' ')" }
    # Keep the markdown report and drop runtime warning lines.
    $keep = $out | Where-Object { $_ -match '^(##|-|\||$)' }
    [void]$report.Add(($keep -join "`n"))
    [void]$report.Add('')
}
$report | Set-Content -Encoding UTF8 'models\inspect-report.md'
Write-Host 'Done. See models\SOURCES.md and models\inspect-report.md.'
