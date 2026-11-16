# Run the Rust hand tracker and the Python one on the same video and compare them.
#
# Run from the repo root, in the activated venv window:
#   .\scripts\try_rust_tracker.ps1 -Video C:\path\to\clip.mp4 [-Device auto|cpu|npu]
# Needs: models in models\ (scripts\fetch_models.ps1), LLVM on PATH (see README), onnxruntime
# (and onnxruntime-qnn for the NPU) in the venv.
# Writes data\try\rust.npz, rust.json (the overlay) and python.npz, then prints how far apart
# the two landmark tracks are and how long each took.
param([Parameter(Mandatory = $true)][string]$Video, [string]$Device = 'auto')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
New-Item -ItemType Directory -Force -Path 'data\try' | Out-Null
$video = (Resolve-Path $Video).Path

Write-Host '>> building the Rust tracker'
cargo build --release -p axonhand-track
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
$track = (Resolve-Path 'target\release\axonhand-track.exe').Path

$paths = python -m axonhand.tools.ort_paths | ConvertFrom-Json
if (-not $paths.library) { throw 'onnxruntime was not found in the venv' }
$extra = ''
if ($paths.npu_name -and $paths.npu_library) {
    $extra = " --npu-name `"$($paths.npu_name)`" --npu-library `"$($paths.npu_library)`""
}

# cmd's pipe carries the raw frames unchanged; PowerShell's pipe can corrupt binary data.
$cmd = "python -m axonhand.tools.decode_frames `"$video`" | `"$track`" data\try\rust.npz" +
    " --models models --device $Device --library `"$($paths.library)`"$extra"
Write-Host ">> Rust tracker ($Device)"
$rust = Measure-Command { cmd /c $cmd | Out-Host }
if ($LASTEXITCODE -ne 0) { throw 'the Rust tracker did not finish; see the message above' }

Write-Host '>> Python tracker (cpu)'
$py = Measure-Command {
    python -m axonhand.hand_model $video 'data\try\python.npz' --models models --device cpu | Out-Host
}
if ($LASTEXITCODE -ne 0) { throw 'the Python tracker did not finish; see the message above' }

Write-Host '>> comparison (Rust against Python)'
python -m axonhand.tools.compare_landmarks 'data\try\python.npz' 'data\try\rust.npz'
if ($LASTEXITCODE -ne 0) { throw 'comparison failed' }
Write-Host ("Rust tracker, including decoding: {0:N1} s. Python tracker on CPU: {1:N1} s." -f $rust.TotalSeconds, $py.TotalSeconds)
