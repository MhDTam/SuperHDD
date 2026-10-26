$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$client=Join-Path $workspace 'target\release\examples\library_client.exe'
$run=[Guid]::NewGuid().ToString('N');$source='D:\SuperHDD-ConfigLab-'+$run+'\App1'
function Call([string[]]$arguments){$out=& $client @arguments 2>&1;if($LASTEXITCODE){throw ($out -join [Environment]::NewLine)};($out -join [Environment]::NewLine)|ConvertFrom-Json}
$before=Call @('state');$euro=@($before.apps|Where-Object id -eq '9be1bad80cbe5c8cebff4c1e977df6e5')[0]
if($euro.phase -ne 'active'){throw 'Expected already active Euro Truck cache'}
$sha=[Security.Cryptography.SHA256]::Create();try{$id=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}
$test=Get-Content -LiteralPath (Join-Path $workspace 'scripts\test-loading-cache.ps1') -Raw;$start=$test.IndexOf('using System;');$end=$test.IndexOf("`n'@",$start);Add-Type -TypeDefinition $test.Substring($start,$end-$start)
[IO.Directory]::CreateDirectory($source)|Out-Null;[IO.File]::WriteAllBytes((Join-Path $source 'asset.bin'),[LoadingRead]::Pattern(91,1048576))
$proof=[ordered]@{passed=$false;runId=$run;fixture=$source;error=$null;checks=@{}}
try{
 $state=Call @('start',$source);$row=@($state.apps|Where-Object id -eq $id)[0];$j=Get-Content -LiteralPath (Join-Path $workspace ('artifacts\library-runtime\'+$id+'\current.json')) -Raw|ConvertFrom-Json
 $original=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\library-runtime\9be1bad80cbe5c8cebff4c1e977df6e5\current.json') -Raw|ConvertFrom-Json
 if($j.cache -ne $original.cache -or $state.allocatedBytes -ne 24999952384 -or $row.allocationBytes -ne 0 -or $row.sharedCapacityBytes -ne 24999952384){throw 'Native shared-capacity invariant failed'}
 $proof.checks.twoNativeItemsShareOne25GbPool=$true
 for($i=0;$i -lt 3;$i++){[void][LoadingRead]::Read((Join-Path $source 'asset.bin'),0,1048576);Start-Sleep -Milliseconds 900}
 $state=Call @('clear',$id);$other=@($state.apps|Where-Object id -eq $euro.id)[0]
 if($other.phase -ne 'active' -or $other.cacheUsedBytes -ne $euro.cacheUsedBytes){throw 'Clearing fixture changed Euro Truck cache'}
 $proof.checks.clearPreservesRealGame=$true
 $state=Call @('stop',$id);$other=@($state.apps|Where-Object id -eq $euro.id)[0]
 if($other.phase -ne 'active' -or $other.sharedCapacityBytes -ne 24999952384){throw 'Stop changed real game capacity'}
 if(-not[LoadingRead]::Equal([IO.File]::ReadAllBytes((Join-Path $source 'asset.bin')),[LoadingRead]::Pattern(91,1048576))){throw 'Restored fixture differs'}
 $proof.checks.stopRestoresFixturePreservesRealGame=$true
 [void](Call @('remove',$id));$proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 try{$state=Call @('state');$row=@($state.apps|Where-Object id -eq $id)[0];if($row){if($row.phase -ne 'stopped'){[void](Call @('stop',$id))};[void](Call @('remove',$id))}}catch{$proof.cleanupError=$_.Exception.Message}
 $proof|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\live-sharing-proof.json') -Encoding utf8
}
$proof|ConvertTo-Json -Depth 7
if(-not $proof.passed){throw 'Live native sharing check failed'}
