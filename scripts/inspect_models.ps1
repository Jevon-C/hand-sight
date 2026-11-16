# Inspect the downloaded hand models on the CPU and NPU and write models\inspect-report.md.
#
# Run from the repo root with the venv active:
#   powershell -ExecutionPolicy Bypass -File .\scripts\inspect_models.ps1
# Run scripts\fetch_models.ps1 first to download the models.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$files = @('palm_detection_mediapipe_2023feb.onnx', 'handpose_estimation_mediapipe_2023feb.onnx')
$report = New-Object System.Collections.ArrayList
foreach ($file in $files) {
    $path = Join-Path 'models' $file
    if (-not (Test-Path $path)) { throw "Missing $path. Run scripts\fetch_models.ps1 first." }
    Write-Host "Inspecting $file"
    # Native tools write warnings to stderr; that must not stop the script.
    $ErrorActionPreference = 'Continue'
    $out = & python -m axonhand.tools.inspect_model $path --runs 50 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "inspect_model failed for ${file}: $($out -join ' ')" }
    # Keep the markdown report and drop runtime warning lines.
    $keep = $out | Where-Object { $_ -match '^(##|-|\||$)' }
    [void]$report.Add(($keep -join "`n"))
    [void]$report.Add('')
}
$report | Set-Content -Encoding UTF8 'models\inspect-report.md'
Write-Host 'Done. See models\inspect-report.md.'
