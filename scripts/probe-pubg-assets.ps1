$ErrorActionPreference='Stop'
$session=Get-Content -Raw 'artifacts\pubg-diagnostics\assets-session.json'|ConvertFrom-Json
if($session.phase -ne 'active' -or -not $session.sharedPool){throw 'Expected active shared asset session'}
function Sample-Hash([string]$path){
 $stream=[IO.File]::OpenRead($path)
 try{
  $bytes=New-Object byte[] (2MB);$count=0
  while($count -lt $bytes.Length){$got=$stream.Read($bytes,$count,$bytes.Length-$count);if(-not $got){throw 'Short asset sample'};$count+=$got}
  $sha=[Security.Cryptography.SHA256]::Create()
  try{([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-','')}finally{$sha.Dispose()}
 }finally{$stream.Dispose()}
}
$file='pakchunk0-WindowsNoEditor_asset_registry.pak'
$original=Sample-Hash (Join-Path $session.backing $file)
$status=Join-Path $session.task 'status.json'
$before=Get-Content -Raw -LiteralPath $status|ConvertFrom-Json
$first=Sample-Hash (Join-Path $session.source $file)
Start-Sleep -Milliseconds 650
$second=Sample-Hash (Join-Path $session.source $file)
Start-Sleep -Milliseconds 1100
$third=Sample-Hash (Join-Path $session.source $file)
Start-Sleep -Milliseconds 650
$after=Get-Content -Raw -LiteralPath $status|ConvertFrom-Json
$proof=[ordered]@{passed=($first -eq $original -and $second -eq $original -and $third -eq $original -and $after.hits -gt $before.hits -and $after.ssdReadBytes -gt $before.ssdReadBytes);scope='Controlled repeated 2 MiB asset sample; not game-loading performance';sampleFile=$file;sha256=$original;allSamplesMatch=($first -eq $original -and $second -eq $original -and $third -eq $original);hitsBefore=$before.hits;hitsAfter=$after.hits;ssdBytesBefore=$before.ssdReadBytes;ssdBytesAfter=$after.ssdReadBytes;residentBytes=$after.occupiedBytes;cacheErrors=$after.cacheErrors;checksumFailures=$after.checksumFailures}
$proof|ConvertTo-Json|Set-Content -Encoding UTF8 'artifacts\pubg-diagnostics\assets-read-proof.json'
$after|ConvertTo-Json -Depth 6|Set-Content -Encoding UTF8 'artifacts\pubg-diagnostics\assets-before-game-status.json'
$proof|ConvertTo-Json
if(-not $proof.passed){throw 'Asset cache sample did not produce matching SSD-served reads'}
