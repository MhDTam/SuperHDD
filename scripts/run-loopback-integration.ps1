# Local native preflight plus disposable protected-payload lifecycle proof.
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$result=[ordered]@{preflightPassed=$false;nativeLifecyclePassed=$false;passed=$false;physicalDAccelerationActive=$false}
try{& (Join-Path $PSScriptRoot 'test-loopback-preflight.ps1');$result.preflightPassed=$true}catch{$result.preflightError=$_.Exception.Message}
try{& (Join-Path $PSScriptRoot 'test-iscsi-cache.ps1') -Handoff -Recovery -NativeInitiator -SectorBytes 512;$result.nativeLifecyclePassed=(Get-Content -LiteralPath (Join-Path $root 'artifacts\iscsi-local-latest.json') -Raw|ConvertFrom-Json).passed}catch{$result.nativeLifecycleError=$_.Exception.Message}
$result.passed=[bool]($result.preflightPassed -and $result.nativeLifecyclePassed)
$result|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $root 'artifacts\loopback-integration.json') -Encoding utf8NoBOM
if(-not $result.passed){throw 'Local native integration checks did not both pass.'}
