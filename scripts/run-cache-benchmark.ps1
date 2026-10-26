param([Parameter(Mandatory=$true)][string]$RunId,[switch]$RecoverOnly)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$env:PSModulePath=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
if($RunId -notmatch '^[a-f0-9]{32}$'){throw 'Invalid benchmark identity'}
$workspace=Split-Path -Parent $PSScriptRoot
$output=Join-Path $workspace ('artifacts\cache-benchmark\runs\'+$RunId)
$request=Get-Content -LiteralPath (Join-Path $output request.json) -Raw|ConvertFrom-Json
if($request.sourceLetter -notmatch '^[A-Z]:$' -or $request.ssdLetter -notmatch '^[A-Z]:$' -or $request.sourceLetter -eq $request.ssdLetter){throw 'Invalid benchmark volumes'}
$sourceRoot=$request.sourceLetter+'\SuperHDD-Benchmark-'+$RunId
$cacheRoot=$request.ssdLetter+'\SuperHDD-Benchmark-'+$RunId
$backing=Join-Path $sourceRoot backing;$mount=Join-Path $sourceRoot selected
$asset=Join-Path $backing archive.bin;$cache=Join-Path $cacheRoot cache.bin;$ssdReference=Join-Path $cacheRoot reference.bin
$ready=Join-Path $output ready;$stop=Join-Path $output stop;$status=Join-Path $output worker-status.json
$cancel=Join-Path $output cancel;$journalFile=Join-Path $output fixture-journal.json
$worker=$null;$workerError=$null;$tag=$null;$started=[DateTime]::UtcNow
function Atomic-Json($path,$value){$temp=$path+'.tmp';[IO.File]::WriteAllText($temp,($value|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false));if([IO.File]::Exists($path)){[IO.File]::Replace($temp,$path,[NullString]::Value)}else{[IO.File]::Move($temp,$path)}}
function Progress-State([string]$phase,[int]$percent,[string]$message='') {Atomic-Json (Join-Path $output state.json) ([ordered]@{runId=$RunId;phase=$phase;percent=$percent;message=$message;updatedUtc=[DateTime]::UtcNow.ToString('o')})}
function Check-Cancel{if(Test-Path -LiteralPath $cancel){throw 'benchmarkCancelled'}}
function Check-Volumes{
 foreach($pair in @(@($request.sourceLetter,$request.sourceVolumeId),@($request.ssdLetter,$request.ssdVolumeId))){$volume=Get-Volume -DriveLetter $pair[0][0] -ErrorAction Stop;if($volume.Path -ne $pair[1] -or $volume.FileSystemType -ne 'NTFS' -or $volume.HealthStatus -ne 'Healthy'){throw 'Benchmark volume identity or health changed'}}
}
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class BenchmarkMount {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)]static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint n,byte[] o,uint l,out uint done,IntPtr x);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool RemoveDirectoryW(string p);
 public static string Tag(string p){using(var h=CreateFileW(p,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){byte[] b=new byte[16384];uint n;if(h.IsInvalid||!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();if(BitConverter.ToUInt32(b,0)!=0xa0000003)throw new Exception("Unexpected mount tag");return Convert.ToBase64String(b,0,(int)n);}}
 public static void Remove(string p,string expected){if(Tag(p)!=expected)throw new Exception("Mount identity changed");if(!RemoveDirectoryW(p))throw new System.ComponentModel.Win32Exception();}
 [DllImport("kernel32.dll")]public static extern ulong GetTickCount64();
}
'@
function Cleanup-Fixture{
 Check-Volumes
 if(Test-Path -LiteralPath $mount){if((Get-Item -LiteralPath $mount -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){if(-not $tag){throw 'Mount identity missing; fixture preserved'};[BenchmarkMount]::Remove($mount,$tag)}}
 foreach($root in @($sourceRoot,$cacheRoot)){
  if(-not (Test-Path -LiteralPath $root)){continue}
  $expected=if($root -eq $sourceRoot){$request.sourceLetter+'\SuperHDD-Benchmark-'+$RunId}else{$request.ssdLetter+'\SuperHDD-Benchmark-'+$RunId}
  if([IO.Path]::GetFullPath($root) -ne $expected -or ((Get-Item -LiteralPath $root).Attributes -band [IO.FileAttributes]::ReparsePoint) -or [IO.File]::ReadAllText((Join-Path $root owner.marker)) -ne $RunId){throw 'Fixture ownership mismatch; preserved'}
 }
 # Exact owned leaves only. Never recursively delete a mounted or arbitrary tree.
 foreach($file in @($asset,$ssdReference,$cache,($cache+'.index'))){if(Test-Path -LiteralPath $file){if((Get-Item -LiteralPath $file).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Linked fixture preserved'};[IO.File]::Delete($file)}}
 foreach($dir in @($backing,$mount)){if(Test-Path -LiteralPath $dir){[IO.Directory]::Delete($dir)}}
 foreach($root in @($sourceRoot,$cacheRoot)){if(Test-Path -LiteralPath $root){$left=@(Get-ChildItem -LiteralPath $root -Force);if($left.Count -ne 1 -or $left[0].Name -ne 'owner.marker'){throw 'Unexpected fixture leaves; owner marker preserved'};[IO.File]::Delete((Join-Path $root owner.marker));[IO.Directory]::Delete($root)}}
}
function Launch($exe,[string[]]$arguments){
 $psi=New-Object Diagnostics.ProcessStartInfo;$psi.FileName=$exe;$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardOutput=$true;$psi.RedirectStandardError=$true
 $psi.Arguments=($arguments|ForEach-Object {'"'+$_+'"'}) -join ' '
 $psi.EnvironmentVariables['SUPERHDD_BENCH_PARENT_PID']=[string]$PID
 $p=[Diagnostics.Process]::Start($psi)
 return $p
}
function Probe([string]$name,[string]$path,[string]$kind=''){
 Check-Cancel
 if(-not $kind){$kind=$parameters.workload}
 $before=Stats
 $p=Launch $probe @('run',$path,[string]$parameters.requests,[string]$parameters.seed,[string]$parameters.threads,$kind,(Join-Path $output ($name+'.csv')))
 $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 try{while(-not $p.WaitForExit(100)){if(Test-Path -LiteralPath $cancel){$p.Kill();$p.WaitForExit();throw 'benchmarkCancelled'}}
  $text=$stdout.GetAwaiter().GetResult();if($p.ExitCode){throw ('Read probe failed: '+$stderr.GetAwaiter().GetResult()+' '+$text)}
  $measurement=$text|ConvertFrom-Json
 }finally{$p.Dispose()}
 $end=[BenchmarkMount]::GetTickCount64();$after=$null
 for($i=0;$i -lt 40;$i++){Check-Cancel;$sample=Stats;if([uint64]$sample.sampledAt -ge $end){$after=$sample;break};Start-Sleep -Milliseconds 100}
 if(-not $after){throw 'Post-read telemetry fence timed out'}
 $delta=[ordered]@{};foreach($key in @('hits','misses','sourceReadBytes','requestedReadBytes','ssdReadBytes','cacheReadApiBytes','fillBytes','queueDrops','cacheErrors','checksumFailures')){$delta[$key]=[long]$after.$key-[long]$before.$key}
 $delta.otherRequestedReadBytes=$delta.requestedReadBytes-([long]$after.benchmarkRequestedReadBytes-[long]$before.benchmarkRequestedReadBytes)
 $delta.requestedReadBytes=[long]$after.benchmarkRequestedReadBytes-[long]$before.benchmarkRequestedReadBytes
 $delta.ssdReadBytes=[long]$after.benchmarkSsdReadBytes-[long]$before.benchmarkSsdReadBytes
 $delta.sourceReadBytes=[long]$after.benchmarkSourceReadBytes-[long]$before.benchmarkSourceReadBytes
 # Windows may issue extra callbacks on a probe-owned handle. Subtract every
 # extra byte from SSD attribution to obtain a conservative payload lower bound.
 $delta.extraCallbackBytes=[Math]::Max(0,$delta.requestedReadBytes-[long]$measurement.bytes)
 $delta.verifiedSsdLowerBoundBytes=[Math]::Max(0,$delta.ssdReadBytes-$delta.extraCallbackBytes)
 if(-not $measurement.passed -or $measurement.errors -or $delta.cacheErrors -or $delta.checksumFailures){throw 'Read verification or cache integrity failed'}
 $entry=[pscustomobject]@{phase=$name;measurement=$measurement;cacheDelta=$delta}
 Atomic-Json (Join-Path $output ($name+'.json')) $entry
 return $entry
}
function Stats{for($i=0;$i -lt 30;$i++){try{return Get-Content -LiteralPath $status -Raw|ConvertFrom-Json}catch{Start-Sleep -Milliseconds 100}};throw 'Worker telemetry unavailable'}
if($RecoverOnly){
 $journal=if(Test-Path -LiteralPath $journalFile){Get-Content -LiteralPath $journalFile -Raw|ConvertFrom-Json}else{$null}
 if($journal.workerPid){$p=Get-Process -Id $journal.workerPid -ErrorAction SilentlyContinue;if($p -and $p.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $journal.workerCreatedFileTime){throw 'Benchmark worker is still running; recovery refused'}}
 $tag=$journal.mountTag;Cleanup-Fixture
 $resultFile=Join-Path $output internal-result.json
 $recovered=if(Test-Path -LiteralPath $resultFile){Get-Content -LiteralPath $resultFile -Raw|ConvertFrom-Json}else{[pscustomobject]@{id=$RunId;startedUtc=$started.ToString('o');parameters=@{profile=$request.profile;workload=$request.workload;pairs=$(if($request.profile -eq 'standard'){6}else{2})};hardware=$request.hardware;build=$null;initialDirect=$null;firstPass=$null;training=@();references=@{};pairs=@();cancelled=$false;error='Benchmark controller interrupted; incomplete run';cleanupComplete=$false}}
 $recovered.cleanupComplete=$true;if(-not $recovered.error){$recovered.error='Interrupted fixture recovered; original run remains invalid'}
 Atomic-Json $resultFile $recovered
 . (Join-Path $PSScriptRoot cache-benchmark-report.ps1)
 [void](Write-BenchmarkReport $recovered $output)
 Progress-State 'recovered' 100;exit
}
$parameters=switch($request.profile){quick{[pscustomobject]@{profile='quick';fixtureMiB=64;requests=128;pairs=2;seed=104729;threads=1;cacheBytes=134217728;protocol='storage-isolated-v1'}}standard{[pscustomobject]@{profile='standard';fixtureMiB=256;requests=512;pairs=6;seed=104729;threads=1;cacheBytes=134217728;protocol='storage-isolated-v1'}}default{throw 'Invalid benchmark profile'}}
$workload=if($request.workload){[string]$request.workload}else{'mixed'};if($workload -notin @('mixed','small','large')){throw 'Invalid benchmark workload'}
$parameters|Add-Member -NotePropertyName workload -NotePropertyValue $workload
$parameters|Add-Member -NotePropertyName workingSetBytes -NotePropertyValue ([long]$parameters.requests*65536)
$parameters|Add-Member -NotePropertyName addressPattern -NotePropertyValue '((operation * 509 + seed) % (fixtureBytes / 65536)) * 65536; unique selected blocks'
$parameters|Add-Member -NotePropertyName requestDistribution -NotePropertyValue $(if($workload -eq 'small'){'100% 4 KiB requests; cache trained with 64 KiB blocks'}elseif($workload -eq 'large'){'100% 64 KiB requests'}else{'25% 4 KiB and 75% 64 KiB requests'})
$result=[ordered]@{id=$RunId;startedUtc=$started.ToString('o');parameters=$parameters;hardware=$request.hardware;build=$null;initialDirect=$null;firstPass=$null;training=@();references=@{};pairs=@();error=$null;cancelled=$false;cleanupComplete=$false}
try{
 Atomic-Json (Join-Path $output runner.json) @{pid=$PID;createdFileTime=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc()}
 Progress-State 'preflight' 1;Check-Volumes;Check-Cancel
 $os=Get-CimInstance Win32_OperatingSystem;$cpu=Get-CimInstance Win32_Processor|Select-Object -First 1
 $result.hardware=[ordered]@{source=$request.hardware.source;ssd=$request.hardware.ssd;os=$os.Caption;osVersion=$os.Version;osBuild=$os.BuildNumber;cpu=$cpu.Name;logicalProcessors=$cpu.NumberOfLogicalProcessors;ramBytes=([long]$os.TotalVisibleMemorySize*1024);dokanVersion=(Get-Item (Join-Path $env:WINDIR 'System32\dokan2.dll')).VersionInfo.FileVersion}
 $bin=Join-Path $workspace 'artifacts\cache-benchmark\bin';$manifest=Get-Content -LiteralPath (Join-Path $bin manifest.json) -Raw|ConvertFrom-Json
 if((Get-FileHash (Join-Path $bin worker.exe)).Hash -ne $manifest.workerSha256 -or (Get-FileHash (Join-Path $bin probe.exe)).Hash -ne $manifest.probeSha256){throw 'Benchmark binary hash mismatch'}
 $result.build=$manifest
 [void][IO.Directory]::CreateDirectory((Join-Path $output bin));[void][IO.Directory]::CreateDirectory((Join-Path $output 'share\source\tools\loading-cache'));[void][IO.Directory]::CreateDirectory((Join-Path $output 'share\source\scripts'))
 foreach($name in @('worker.exe','probe.exe')){[IO.File]::Copy((Join-Path $bin $name),(Join-Path (Join-Path $output bin) $name),$false)}
 foreach($property in $manifest.sources.PSObject.Properties){$file=Join-Path $workspace ('tools\loading-cache\'+$property.Name);if((Get-FileHash $file).Hash -ne $property.Value){throw 'Benchmark source changed after compilation'};[IO.File]::Copy($file,(Join-Path (Join-Path $output 'share\source\tools\loading-cache') $property.Name),$false)}
 foreach($name in @('run-cache-benchmark.ps1','cache-benchmark-report.ps1','build-cache-benchmark.ps1')){[IO.File]::Copy((Join-Path $PSScriptRoot $name),(Join-Path (Join-Path $output 'share\source\scripts') $name),$false)}
 $method=Join-Path $workspace 'docs\architecture\cache-benchmark.md';if(Test-Path -LiteralPath $method){[IO.File]::Copy($method,(Join-Path $output 'share\METHOD.md'),$false)}
 $probe=Join-Path $output 'bin\probe.exe';$workerExe=Join-Path $output 'bin\worker.exe'
 foreach($root in @($sourceRoot,$cacheRoot)){if(Test-Path -LiteralPath $root){throw 'Fixture already exists; refusing reuse'};[void][IO.Directory]::CreateDirectory($root);[IO.File]::WriteAllText((Join-Path $root owner.marker),$RunId)}
 [void][IO.Directory]::CreateDirectory($backing);[void][IO.Directory]::CreateDirectory($mount)
 Atomic-Json $journalFile @{source=$sourceRoot;cache=$cacheRoot;workerPid=$null;mountTag=$null}
 Progress-State 'creatingFixture' 4
 $p=Launch $probe @('make',$asset,[string]$parameters.fixtureMiB);$stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 while(-not $p.WaitForExit(100)){if(Test-Path -LiteralPath $cancel){$p.Kill();$p.WaitForExit();throw 'benchmarkCancelled'}}
 if($p.ExitCode){throw ('Fixture creation failed: '+$stderr.GetAwaiter().GetResult())};$p.Dispose();Check-Cancel
 $p=Launch $probe @('make',$ssdReference,[string]$parameters.fixtureMiB);$stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 while(-not $p.WaitForExit(100)){if(Test-Path -LiteralPath $cancel){$p.Kill();$p.WaitForExit();throw 'benchmarkCancelled'}}
 if($p.ExitCode){throw ('SSD reference creation failed: '+$stderr.GetAwaiter().GetResult())};$p.Dispose()
 $worker=Launch $workerExe @('run',$backing,$mount,$cache,[string]$parameters.cacheBytes,$ready,$stop,$status,$RunId)
 $workerError=$worker.StandardError.ReadToEndAsync();$workerOutput=$worker.StandardOutput.ReadToEndAsync()
 Atomic-Json $journalFile @{source=$sourceRoot;cache=$cacheRoot;workerPid=$worker.Id;workerCreatedFileTime=$worker.StartTime.ToUniversalTime().ToFileTimeUtc();mountTag=$null}
 for($i=0;$i -lt 150;$i++){Check-Cancel;if($worker.HasExited){throw ('Worker exited: '+$workerError.GetAwaiter().GetResult())};if(Test-Path -LiteralPath $ready){break};Start-Sleep -Milliseconds 100}
 if(-not (Test-Path -LiteralPath $ready) -or [IO.File]::ReadAllText($status+'.isolation') -ne 'source-and-cache-unbuffered-v1'){throw 'Storage isolation was not enabled'}
 $tag=[BenchmarkMount]::Tag($mount)
 Atomic-Json $journalFile @{source=$sourceRoot;cache=$cacheRoot;workerPid=$worker.Id;workerCreatedFileTime=$worker.StartTime.ToUniversalTime().ToFileTimeUtc();mountTag=$tag}
 Progress-State 'firstPass' 10
 $result.initialDirect=Probe 'initial-direct' $asset
 $result.firstPass=Probe 'cache-first-pass' (Join-Path $mount archive.bin)
 Progress-State 'warming' 18
 foreach($i in 1..2){Start-Sleep -Milliseconds 650;$trainingKind=if($i%2){'train'}else{'train-rotated'};$result.training+=Probe ('training-'+$i) (Join-Path $mount archive.bin) $trainingKind}
 $warm=Probe 'warm-validation' (Join-Path $mount archive.bin)
 for($i=0;$i -lt 3 -and $warm.cacheDelta.verifiedSsdLowerBoundBytes -lt $warm.measurement.bytes*.95;$i++){Start-Sleep -Milliseconds 650;$trainingKind=if($i%2){'train-rotated'}else{'train'};$result.training+=Probe ('extra-training-'+$i) (Join-Path $mount archive.bin) $trainingKind;$warm=Probe ('extra-validation-'+$i) (Join-Path $mount archive.bin)}
 if($warm.cacheDelta.verifiedSsdLowerBoundBytes -lt $warm.measurement.bytes*.95){throw 'Warm cache did not serve at least 95% of validation bytes'}
 $result.references.ssdDirect=Probe 'reference-ssd-direct' $ssdReference
 $result.references.windowsWarmup=Probe 'reference-windows-warmup' $asset 'buffered-train'
 $referenceKind=if($workload -eq 'mixed'){'buffered'}else{'buffered-'+$workload}
 $result.references.windowsWarm=Probe 'reference-windows-warm' $asset $referenceKind
 for($i=1;$i -le $parameters.pairs;$i++){
  Progress-State 'measuring' ([int](25+60*($i-1)/$parameters.pairs)) ('Pair '+$i+' / '+$parameters.pairs)
  $order=if($i%2){@('direct','cached')}else{@('cached','direct')};$pair=[ordered]@{index=$i;order=$order;direct=$null;cached=$null}
  foreach($arm in $order){Check-Cancel;$path=if($arm -eq 'direct'){$asset}else{Join-Path $mount archive.bin};$pair[$arm]=Probe ('pair-'+$i+'-'+$arm) $path}
  $result.pairs+=[pscustomobject]$pair
 }
}catch{$result.cancelled=$_.Exception.Message -like '*benchmarkCancelled*';if(-not $result.cancelled){$result.error=$_.Exception.Message.Replace($workspace,'<workspace>')}}finally{
 Progress-State 'cleanup' 90
 try{
  if($worker -and -not $worker.HasExited){[IO.File]::WriteAllText($stop,'stop');if(-not $worker.WaitForExit(30000)){throw 'Worker did not stop; owned fixture preserved'}}
  Cleanup-Fixture;$result.cleanupComplete=$true
 }catch{$result.error=[string]$result.error+'; cleanup: '+$_.Exception.Message.Replace($workspace,'<workspace>')}
 if($worker){$worker.Dispose()}
 Atomic-Json (Join-Path $output internal-result.json) $result
 $reportScript=Join-Path $output 'share\source\scripts\cache-benchmark-report.ps1'
 if(-not (Test-Path -LiteralPath $reportScript)){$reportScript=Join-Path $PSScriptRoot cache-benchmark-report.ps1}
 . $reportScript
 try{$public=Write-BenchmarkReport $result $output;$phase=if($result.cancelled){'cancelled'}elseif($public.valid){'complete'}else{'failed'};$message=if(-not $public.valid -and -not $result.error -and -not $result.cancelled){'Evidence validation failed; inspect the report and raw samples'}else{$result.error};Progress-State $phase 100 $message}catch{Progress-State 'failed' 100 ('Report failed: '+$_.Exception.Message)}
}
