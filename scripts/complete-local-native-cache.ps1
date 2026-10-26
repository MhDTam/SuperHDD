# One approved elevated run: current-binary lab proof first, then the reviewed
# physical D/M allocation. The physical installer refuses an incomplete proof.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
if(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json'){throw 'Existing cache ownership is preserved; inspect it before a fresh complete run.'}
& (Join-Path $PSScriptRoot 'run-loopback-runtime-lab.ps1') -Scm
$proof=Get-Content -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-lab.json') -Raw|ConvertFrom-Json
if(-not $proof.passed){throw 'Physical activation is blocked by the current lab result.'}
& (Join-Path $PSScriptRoot 'enable-local-native-cache.ps1')
