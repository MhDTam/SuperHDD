$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
# Explicit run IDs created by this change. Never enumerate arbitrary D/E roots.
$runs=@('21166f735427438dbd4a0230b3889e2e','9d00e46cf89b4deba3eee2181d502e66','a6e4db0eb2704e3291616d6db6462e81','e5d945d951d94a369ed3a317a672a78b','8fb4a4cb5da041459453eb3e0cb6e2db','0e38628e630a442a8d6bb2568b0018ab','5f9074066b9f4133b2aca02000334ea9','d13cb869d516478fa0440011ecff071d','53c0cc870c244514a307f56da61fc41f')
$live=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\live-sharing-proof.json') -Raw|ConvertFrom-Json
if(-not $live.passed){throw 'Keep live fixture for review until its verification passes'}
$runs+=$live.runId
$activeCaches=@(Get-ChildItem -LiteralPath (Join-Path $workspace 'artifacts\library-runtime') -Directory|ForEach-Object {$p=Join-Path $_.FullName 'current.json';if(Test-Path -LiteralPath $p){$j=Get-Content -LiteralPath $p -Raw|ConvertFrom-Json;if(-not $j.restored){$j.cache}}})
$removed=@();$preserved=@()
function Remove-Fixture([string]$path){
 $full=[IO.Path]::GetFullPath($path)
 if($full -ne $path -or $full -notmatch '^[DE]:\\SuperHDD-(ConfigLab|FolderLab)-[a-f0-9]{32}$'){throw 'Unexpected fixture target'}
 if(-not(Test-Path -LiteralPath $full)){return}
 if((Get-Item -LiteralPath $full -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Fixture root is a mount; preserve it'}
 $links=@(Get-ChildItem -LiteralPath $full -Force -Recurse|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint})
 if($links.Count){$script:preserved+=$full;return}
 Remove-Item -LiteralPath $full -Recurse -Force;$script:removed+=$full
}
foreach($run in $runs){if($run -notmatch '^[a-f0-9]{32}$'){throw 'Invalid test ID'};foreach($letter in @('D:','E:')){Remove-Fixture ($letter+'\SuperHDD-ConfigLab-'+$run)}}
foreach($run in @('85fcf6031a3a4fe29090fe38d71bddcb','389d16d5944d4ab7b9d94dfe1ef1e0d5')){Remove-Fixture ('D:\SuperHDD-FolderLab-'+$run)}
foreach($letter in @('M:','F:')){foreach($run in @($runs+('e'*32))){
 $pool=$letter+'\SuperHDD-Cache\1C00B670BA7219A3\pools\'+$run+'\cache.bin'
 if($activeCaches -contains $pool){$preserved+=$pool;continue}
 foreach($file in @($pool,($pool+'.index'))){
  if(-not(Test-Path -LiteralPath $file)){continue}
  if([IO.Path]::GetFullPath($file) -ne $file -or $file -notmatch '^[MF]:\\SuperHDD-Cache\\1C00B670BA7219A3\\pools\\[a-f0-9]{32}\\cache\.bin(\.index)?$'){throw 'Unexpected pool target'}
  $ancestor=Split-Path -Parent $file;while($ancestor.Length -gt 3){if((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unsafe pool ancestry'};$ancestor=Split-Path -Parent $ancestor}
  if((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Pool file is a link'}
  Remove-Item -LiteralPath $file;$removed+=$file
 }
}}
[pscustomobject]@{removed=$removed;preserved=$preserved}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\fixture-cleanup.json') -Encoding utf8
