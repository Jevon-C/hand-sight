# Run the real hand model on one video and draw what it found.
#
# Run from the repo root:  .\scripts\try_hand_model.ps1 -Video C:\path\to\clip.mp4
# Needs: the virtual environment active and the model files in models\ (scripts\fetch_models.ps1).
# Writes data\try\landmarks.npz, data\try\sheet.png (a grid of frames with the hand drawn on)
# and data\try\drawn.mp4 (the whole video with the drawing). Open sheet.png to judge the result.
param([Parameter(Mandatory = $true)][string]$Video, [string]$Device = 'auto')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
New-Item -ItemType Directory -Force -Path 'data\try' | Out-Null
python -m axonhand.hand_model $Video 'data\try\landmarks.npz' --models models --device $Device
if ($LASTEXITCODE -ne 0) { throw 'the hand model did not finish; see the message above' }
python -m axonhand.tools.draw_landmarks $Video 'data\try\landmarks.npz' 'data\try\sheet.png' --video-out 'data\try\drawn.mp4' --frames 9
if ($LASTEXITCODE -ne 0) { throw 'drawing failed' }
Write-Host 'Done. Open data\try\sheet.png'
