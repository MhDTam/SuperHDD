param([string]$SourceLetter='D:',[string]$SsdLetter='M:',[UInt32]$Files=16,[UInt32]$FileMiB=192,[UInt64]$PoolMiB=2048,[UInt32]$Threads=4,[UInt32]$Seed=7,[string[]]$Variants=@('baseline','candidate'))
# Before/after loading replay on new disposable files only. Each variant is the
# same worker source built in storage-isolation mode: source and SSD reads are
# unbuffered for the replay client, so Windows' RAM cache cannot hide the HDD.
# Passes: direct HDD, mounted cold, repeated loads, then a worker restart (as
# after relaunching the game or rebooting) followed by another identical load.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$output=Join-Path $workspace ('artifacts\loading-replay\'+$run);[void][IO.Directory]::CreateDirectory($output)
if($SourceLetter -notmatch '^[A-Z]:$' -or $SsdLetter -notmatch '^[A-Z]:$' -or $SourceLetter -eq $SsdLetter){throw 'Choose distinct source and SSD letters'}
$labRoot=$SourceLetter+'\SuperHDD-ReplayLab-'+$run;$cacheRoot=$SsdLetter+'\SuperHDD-ReplayLab-'+$run
$backing=Join-Path $labRoot backing;$mount=Join-Path $labRoot selected
foreach($root in @($labRoot,$cacheRoot)){if(Test-Path -LiteralPath $root){throw 'Lab root must be new'}}
$needed=[UInt64]$Files*$FileMiB*1MB
if((Get-Volume -DriveLetter $SourceLetter[0]).SizeRemaining -lt $needed+10GB){throw 'Insufficient HDD space for the fixture'}
if((Get-Volume -DriveLetter $SsdLetter[0]).SizeRemaining -lt [UInt64]$PoolMiB*1MB*[UInt64]$Variants.Count+20GB){throw 'Insufficient SSD space for the pools'}
# --- build ---------------------------------------------------------------
$vs=& 'C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe' -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$compiler=(Get-ChildItem -LiteralPath (Join-Path $vs 'VC\Tools\MSVC') -Directory|Sort-Object Name -Descending|Select-Object -First 1).FullName
$sdk='C:\Program Files (x86)\Windows Kits\10';$version='10.0.26100.0'
$env:INCLUDE=(@((Join-Path $compiler 'include'),(Join-Path $sdk "Include\$version\ucrt"),(Join-Path $sdk "Include\$version\shared"),(Join-Path $sdk "Include\$version\um")) -join ';')
$env:LIB=(@((Join-Path $compiler 'lib\x64'),(Join-Path $sdk "Lib\$version\ucrt\x64"),(Join-Path $sdk "Lib\$version\um\x64")) -join ';')
$cl=Join-Path $compiler 'bin\Hostx64\x64\cl.exe'
$sources=@{baseline=(Join-Path $workspace 'artifacts\loading-replay\baseline-src');candidate=(Join-Path $workspace 'tools\loading-cache')}
$bin=Join-Path $output bin;[void][IO.Directory]::CreateDirectory($bin)
& $cl /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX /DUNICODE /D_UNICODE ('/Fo'+(Join-Path $bin 'replay.obj')) (Join-Path $workspace 'tools\loading-cache\loading-replay.cpp') /link ('/OUT:'+(Join-Path $bin 'loading-replay.exe'))|Out-Null
if($LASTEXITCODE){throw 'Replay build failed'}
$replay=Join-Path $bin 'loading-replay.exe'
foreach($variant in $Variants){
 & $cl /nologo /std:c++17 /O2 /MT /EHsc /W4 /WX /DUNICODE /D_UNICODE /DSUPERHDD_STORAGE_BENCHMARK ('/I'+(Join-Path $workspace '.tools\dokan-sdk\2.3.1')) ('/Fo'+(Join-Path $bin ($variant+'.obj'))) (Join-Path $sources[$variant] 'worker.cpp') /link ('/OUT:'+(Join-Path $bin ($variant+'-worker.exe'))) advapi32.lib bcrypt.lib|Out-Null
 if($LASTEXITCODE){throw ('Worker build failed: '+$variant)}
}
# --- helpers -------------------------------------------------------------
function Replay([string]$dir){
 $text=& $replay run $dir $Files $FileMiB $Seed $Threads
 if($LASTEXITCODE){throw ('Replay failed: '+$text)}
 return $text|ConvertFrom-Json
}
function Stats($status){for($i=0;$i -lt 50;$i++){try{return Get-Content -LiteralPath $status -Raw|ConvertFrom-Json}catch{Start-Sleep -Milliseconds 100}};throw 'No worker telemetry'}
function Drain($status){$last=-1;$stable=0;for($i=0;$i -lt 600 -and $stable -lt 4;$i++){Start-Sleep -Milliseconds 500;$s=Stats $status;if($s.fillBytes -eq $last){$stable++}else{$stable=0;$last=$s.fillBytes}};return Stats $status}
function Remove-Mount{
 if(-not(Test-Path -LiteralPath $mount)){return}
 if(@(Get-ChildItem -LiteralPath $mount -Force -ErrorAction SilentlyContinue).Count -and -not((Get-Item -LiteralPath $mount -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Mount directory unexpectedly holds files; preserved'}
 [IO.Directory]::Delete($mount,$false)
}
$script:worker=$null
function Start-Worker($variant,$variantDir,$cache){
 Remove-Mount;[void][IO.Directory]::CreateDirectory($mount)
 $ready=Join-Path $variantDir ready;$stop=Join-Path $variantDir stop;$status=Join-Path $variantDir status.json
 foreach($f in @($ready,$stop)){if(Test-Path -LiteralPath $f){[IO.File]::Delete($f)}}
 $psi=New-Object Diagnostics.ProcessStartInfo;$psi.FileName=Join-Path $bin ($variant+'-worker.exe');$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true
 $psi.EnvironmentVariables['SUPERHDD_BENCH_PARENT_PID']=[string]$PID
 $psi.Arguments=(@('run',$backing,$mount,$cache,[string]([UInt64]$PoolMiB*1MB),$ready,$stop,$status,('b'*32))|ForEach-Object {'"'+$_+'"'}) -join ' '
 $script:worker=[Diagnostics.Process]::Start($psi);$script:workerErrors=$script:worker.StandardError.ReadToEndAsync()
 for($i=0;$i -lt 300;$i++){if($script:worker.HasExited){throw ('Worker exited: '+$script:workerErrors.GetAwaiter().GetResult())};if(Test-Path -LiteralPath $ready){return};Start-Sleep -Milliseconds 100}
 throw 'Worker startup timed out'
}
function Stop-Worker($variantDir){
 if(-not $script:worker){return}
 if(-not $script:worker.HasExited){[IO.File]::WriteAllText((Join-Path $variantDir stop),'stop');if(-not $script:worker.WaitForExit(60000)){$script:worker.Kill();$script:worker.WaitForExit()}}
 $script:worker=$null;Start-Sleep -Milliseconds 500;Remove-Mount
}
function Delta($before,$after,$timing,[string]$name){
 [ordered]@{pass=$name;seconds=[Math]::Round($timing.seconds,3);MiBPerSecond=[Math]::Round($timing.MiBPerSecond,1);p50Ms=$timing.p50Ms;p95Ms=$timing.p95Ms;p99Ms=$timing.p99Ms;maxMs=$timing.maxMs;
  ssdMiB=[Math]::Round(($after.benchmarkSsdReadBytes-$before.benchmarkSsdReadBytes)/1MB,1);hddMiB=[Math]::Round(($after.benchmarkSourceReadBytes-$before.benchmarkSourceReadBytes)/1MB,1);
  hitPercent=if($after.benchmarkRequestedReadBytes-$before.benchmarkRequestedReadBytes){[Math]::Round(100*($after.benchmarkSsdReadBytes-$before.benchmarkSsdReadBytes)/($after.benchmarkRequestedReadBytes-$before.benchmarkRequestedReadBytes),1)}else{0};
  residentMiB=[Math]::Round($after.occupiedBytes/1MB,1);queueDrops=$after.queueDrops-$before.queueDrops;lockBypasses=$after.cacheLockBypasses-$before.cacheLockBypasses;verified=$timing.passed}
}
# --- run -----------------------------------------------------------------
$result=[ordered]@{runId=$run;scope='Disposable fixture files only; storage-isolated worker builds; not a real game or a frame-time measurement';parameters=[ordered]@{files=$Files;fileMiB=$FileMiB;poolMiB=$PoolMiB;threads=$Threads;seed=$Seed;source=$SourceLetter;ssd=$SsdLetter};direct=@();variants=[ordered]@{};passed=$false;error=$null}
try{
 foreach($root in @($labRoot,$cacheRoot)){[void][IO.Directory]::CreateDirectory($root);[IO.File]::WriteAllText((Join-Path $root owner.marker),$run)}
 [void][IO.Directory]::CreateDirectory($backing)
 $made=& $replay make $backing $Files $FileMiB;if($LASTEXITCODE){throw ('Fixture creation failed: '+$made)}
 foreach($i in 1..2){$t=Replay $backing;$result.direct+=[ordered]@{pass=('direct HDD '+$i);seconds=[Math]::Round($t.seconds,3);MiBPerSecond=[Math]::Round($t.MiBPerSecond,1);p50Ms=$t.p50Ms;p95Ms=$t.p95Ms;p99Ms=$t.p99Ms;maxMs=$t.maxMs;verified=$t.passed}}
 foreach($variant in $Variants){
  $variantDir=Join-Path $output $variant;[void][IO.Directory]::CreateDirectory((Join-Path $variantDir bin));Copy-Item -LiteralPath $replay -Destination (Join-Path $variantDir 'bin\probe.exe')
  $cache=Join-Path $cacheRoot ($variant+'.bin');$status=Join-Path $variantDir status.json;$passes=@()
  Start-Worker $variant $variantDir $cache
  $mountReplay=Join-Path $variantDir 'bin\probe.exe'
  foreach($name in @('1st load (cold cache)','2nd load','3rd load')){$before=Stats $status;$t=& $mountReplay run $mount $Files $FileMiB $Seed $Threads|ConvertFrom-Json;$after=Drain $status;$passes+=Delta $before $after $t $name}
  Stop-Worker $variantDir
  Start-Worker $variant $variantDir $cache
  $before=Stats $status;$t=& $mountReplay run $mount $Files $FileMiB $Seed $Threads|ConvertFrom-Json;$after=Drain $status;$passes+=Delta $before $after $t 'after restart'
  Stop-Worker $variantDir
  $result.variants[$variant]=$passes
 }
 $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
 try{if($script:worker){Stop-Worker (Join-Path $output 'cleanup')}}catch{$result.cleanupError=$_.Exception.Message}
 try{
  Remove-Mount
  # Exact owned leaves only; never a recursive delete of a mounted tree.
  foreach($i in 0..($Files-1)){$f=Join-Path $backing ('pak{0:D2}.pak' -f $i);if(Test-Path -LiteralPath $f){[IO.File]::Delete($f)}}
  foreach($variant in $Variants){foreach($f in @((Join-Path $cacheRoot ($variant+'.bin')),(Join-Path $cacheRoot ($variant+'.bin.index')))){if(Test-Path -LiteralPath $f){[IO.File]::Delete($f)}}}
  if(Test-Path -LiteralPath $backing){[IO.Directory]::Delete($backing,$false)}
  foreach($root in @($labRoot,$cacheRoot)){if(Test-Path -LiteralPath $root){[IO.File]::Delete((Join-Path $root owner.marker));[IO.Directory]::Delete($root,$false)}}
  $result.fixtureRemoved=-not(Test-Path -LiteralPath $labRoot) -and -not(Test-Path -LiteralPath $cacheRoot)
 }catch{$result.cleanupError=$_.Exception.Message;$result.fixtureRemoved=$false}
 $result|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $output result.json) -Encoding UTF8
}
$result|ConvertTo-Json -Depth 8
if(-not $result.passed){throw ('Loading replay failed; see '+$output)}
