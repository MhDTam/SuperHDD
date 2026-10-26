$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
if(Get-Process -Name 'folder-worker' -ErrorAction SilentlyContinue){throw 'Finish audit workers before cleaning fixtures'}
$removed=[Collections.Generic.List[string]]::new()
function Remove-TestRoot([string]$path){
 $full=[IO.Path]::GetFullPath($path)
 if($full -ne $path -or $full -notmatch '^[DE]:\\SuperHDD-(FolderLab|ConfigLab|LatencyLab)-[a-f0-9]{32}$'){throw 'Unexpected test root'}
 if(-not(Test-Path -LiteralPath $full)){return}
 if((Get-Item -LiteralPath $full -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Preserve linked root'}
 if(@(Get-ChildItem -LiteralPath $full -Recurse -Force|Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'Preserve fixture with a mounted child'}
 Remove-Item -LiteralPath $full -Recurse -Force;$removed.Add($full)
}
foreach($id in @('b1b0386f91834522a54e55b0afde1311','607b8309db2e4245b031754cc9a4648c')){Remove-TestRoot ('D:\SuperHDD-FolderLab-'+$id)}
foreach($id in @('8b0b574780d447c8895f0ab52332c411','0c2e986b131c48aa91900064e4a0ed97','d7689a7d82c84618839f1f81383d8806')){foreach($letter in @('D:','E:')){Remove-TestRoot ($letter+'\SuperHDD-ConfigLab-'+$id)}}
$latencyIds=@('e291870f26504f1d9ae57260e451a0d2','ee2c356abff14bb4bf70961752160e0d','6154be1c40c44f8591b280fc4c89986d')
foreach($id in $latencyIds){
 $proofPath=Join-Path $workspace ('artifacts\loading-cache\latency-'+$id+'\result.json')
 $proof=Get-Content -LiteralPath $proofPath -Raw|ConvertFrom-Json
 if(-not $proof.passed -or $proof.fixture -ne ('D:\SuperHDD-LatencyLab-'+$id)){throw 'Latency fixture not verified'}
 Remove-TestRoot $proof.fixture
 $artifactRoot=Split-Path -Parent $proofPath
 foreach($file in @(Get-ChildItem -LiteralPath $artifactRoot -Recurse -File|Where-Object {$_.Name -in @('cache.bin','cache.bin.index')})){
  if(-not $file.FullName.StartsWith($artifactRoot+'\',[StringComparison]::OrdinalIgnoreCase) -or $file.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unexpected cache path'}
  Remove-Item -LiteralPath $file.FullName;$removed.Add($file.FullName)
 }
}
$auditRoot=Join-Path $workspace 'artifacts\audit-performance'
foreach($dir in @(Get-ChildItem -LiteralPath $auditRoot -Directory|Where-Object {$_.Name -match '^bench-(baseline|candidate)-[123]$|^unit(-final|-preliminary)?-[a-f0-9]{32}$'})){
 if($dir.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unexpected benchmark root'}
 foreach($name in @('cache.bin','cache.bin.index','pool.bin','pool.bin.index')){$file=Join-Path $dir.FullName $name;if(Test-Path -LiteralPath $file){if((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked benchmark file'};Remove-Item -LiteralPath $file;$removed.Add($file)}}
}
# These two pool IDs are named by the current session proof, never the user's D/M pool.
foreach($pair in @(@('M:','8b0b574780d447c8895f0ab52332c411'),@('F:',('e'*32)))){
 $pool=$pair[0]+'\SuperHDD-Cache\1C00B670BA7219A3\pools\'+$pair[1]+'\cache.bin'
 foreach($file in @($pool,($pool+'.index'))){if(-not(Test-Path -LiteralPath $file)){continue};if([IO.Path]::GetFullPath($file) -ne $file -or $file -notmatch '^[MF]:\\SuperHDD-Cache\\1C00B670BA7219A3\\pools\\[a-f0-9]{32}\\cache\.bin(\.index)?$'){throw 'Unexpected pool'};$ancestor=$file;while($ancestor.Length -gt 3){if((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Unsafe ancestry'};$ancestor=Split-Path -Parent $ancestor};Remove-Item -LiteralPath $file;$removed.Add($file)}
}
@{removed=$removed.ToArray();proofsPreserved=$true;actualGameChanged=$false}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $auditRoot 'fixture-cleanup.json') -Encoding utf8
'Removed '+$removed.Count+' owned test roots/cache files; evidence and user data preserved.'
