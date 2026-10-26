$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N');$items=@();$checks=[ordered]@{}
$launcher=Join-Path $workspace 'tools\loading-cache\schedule-session.ps1'
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class LoadingRead {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32")]static extern IntPtr VirtualAlloc(IntPtr a,UIntPtr n,uint t,uint p);
 [DllImport("kernel32")]static extern bool VirtualFree(IntPtr a,UIntPtr n,uint t);
 [DllImport("kernel32",SetLastError=true)]static extern bool ReadFile(SafeFileHandle h,IntPtr b,uint n,out uint done,IntPtr o);
 [DllImport("kernel32",SetLastError=true)]static extern bool SetFilePointerEx(SafeFileHandle h,long p,out long result,uint method);
 public static byte[] Read(string p,int offset,int length){using(var h=CreateFileW(p,0x80000000,7,IntPtr.Zero,3,0x20000000,IntPtr.Zero)){if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();IntPtr b=VirtualAlloc(IntPtr.Zero,(UIntPtr)length,0x3000,4);try{uint n;long at;if(!SetFilePointerEx(h,offset,out at,0)||!ReadFile(h,b,(uint)length,out n,IntPtr.Zero)||n!=length)throw new IOException("Read failed "+Marshal.GetLastWin32Error());byte[] bytes=new byte[n];Marshal.Copy(b,bytes,0,(int)n);return bytes;}finally{VirtualFree(b,UIntPtr.Zero,0x8000);}}}
 public static byte[] Pattern(int seed,int size){byte[] b=new byte[size];for(int i=0;i<b.Length;i++)b[i]=(byte)((i*17+seed)%251);return b;}
 public static bool Equal(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
}
'@
function Journal($a){$path=Join-Path $a.runtime 'current.json';if(Test-Path -LiteralPath $path){Get-Content -LiteralPath $path -Raw|ConvertFrom-Json}}
function Stats($a){$j=Journal $a;Get-Content -LiteralPath (Join-Path $j.task 'status.json') -Raw|ConvertFrom-Json}
function Wait-Until([scriptblock]$test){for($i=0;$i -lt 300;$i++){try{if(& $test){return}}catch{};Start-Sleep -Milliseconds 100};throw 'Loading cache state timeout'}
function Start-App($a){$a.started=$true;& $launcher -Action start -AppId $a.id -Source $a.source -CapacityBytes 8388608 -Lab|Out-Null;for($attempt=0;$attempt -lt 400;$attempt++){if(Test-Path -LiteralPath (Join-Path $a.runtime 'startup-error.json')){throw (Get-Content -LiteralPath (Join-Path $a.runtime 'startup-error.json') -Raw)};if((Journal $a).phase -eq 'active'){return};Start-Sleep -Milliseconds 100};throw 'Host did not become active'}
function Control($a,$name,$value='control'){$j=Journal $a;[IO.File]::WriteAllText((Join-Path $j.task $name),$value)}
function Read-All($a){$p=Join-Path $a.source 'asset.bin';if(-not[LoadingRead]::Equal([LoadingRead]::Read($p,0,8388608),[LoadingRead]::Pattern($a.seed,8388608))){throw 'Wrong source/cache bytes'}}
$proof=[ordered]@{runId=$run;passed=$false;checks=$checks;error=$null;fixtures=@()}
try{
 $devices=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\diagnostics\configuration-devices.json') -Raw|ConvertFrom-Json
 foreach($seed in @(1,2,3)){
  $letter=if($seed -eq 3){'E:'}else{'D:'};$ssd=if($seed -eq 3){'F:'}else{'M:'};$source=$letter+'\SuperHDD-ConfigLab-'+$run+'\App'+$seed
  $sha=[Security.Cryptography.SHA256]::Create();try{$id=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}
  [IO.Directory]::CreateDirectory($source)|Out-Null;[IO.File]::WriteAllBytes((Join-Path $source 'asset.bin'),[LoadingRead]::Pattern($seed,8388608))
  $runtime=Join-Path $workspace ('artifacts\library-runtime\'+$id);[IO.Directory]::CreateDirectory($runtime)|Out-Null
  $sd=@($devices.disks|Where-Object {@($_.volumes.letter) -contains $letter})[0];$cd=@($devices.disks|Where-Object {@($_.volumes.letter) -contains $ssd})[0]
  $request=@{sourceDiskId=$sd.id;sourceLetter=$letter;ssdDiskId=$cd.id;ssdLetter=$ssd;capacityBytes=8388608;scope=@{mode='selected';folders=@($source)}}
  $config=if($seed -eq 3){('e'*32)}else{$run}
  @{configurationId=$config;request=$request;sourceVolumeId=(Get-Volume -DriveLetter $letter[0]).Path;ssdVolumeId=(Get-Volume -DriveLetter $ssd[0]).Path}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $runtime 'session-plan.json')
  $items+=@{id=$id;source=$source;runtime=$runtime;seed=$seed;ssd=$ssd}
 }
 $proof.fixtures=$items;$a=$items[0];$b=$items[1];$c=$items[2]
 foreach($app in $items){Start-App $app}
 if((Journal $a).cache -ne (Journal $b).cache -or (Journal $c).cache -eq (Journal $a).cache){throw 'Pool ownership mismatch'}
 if((Stats $a).capacityBytes -ne 8388608 -or (Stats $b).capacityBytes -ne 8388608){throw 'Fixed per-game quota remains'}
 $checks.onePoolWithoutPerGameQuotas=$true
 $probe=Join-Path $a.source 'probe.bin';[IO.File]::WriteAllBytes($probe,[LoadingRead]::Pattern(45,1048576));Start-Sleep -Seconds 1;$before=Stats $a;[void][LoadingRead]::Read($probe,0,4096);Wait-Until {(Stats $a).sourceReadBytes -gt $before.sourceReadBytes};$after=Stats $a;$proof.partialReadSourceDelta=$after.sourceReadBytes-$before.sourceReadBytes;$proof.partialReadIncomingDelta=$after.requestedReadBytes-$before.requestedReadBytes;if($proof.partialReadSourceDelta -gt $proof.partialReadIncomingDelta){throw 'Source read amplification inside worker'};$checks.sourceBytesDoNotExceedIncomingDokanRequests=$true
 # Free pool space is filled by the first load; the repeat is served from SSD.
 Read-All $a;Wait-Until {(Stats $a).occupiedBytes -ge 4194304};$checks.firstPassFillsFreePoolSpace=$true
 $warm=Stats $a;Read-All $a;Wait-Until {(Stats $a).ssdReadBytes -gt $warm.ssdReadBytes}
 $checks.repeatedLoadIsServedFromSsd=$true
 Read-All $b;Start-Sleep -Milliseconds 700;Read-All $b;Wait-Until {(Stats $b).occupiedBytes -gt 0}
 Wait-Until {((Stats $a).occupiedBytes+(Stats $b).occupiedBytes) -le 8388608};if((Stats $b).poolOccupiedBytes -gt 8388608){throw 'Shared pool overfilled'}
 $checks.residencyBoundedAcrossMounts=$true
 Read-All $c;Start-Sleep -Milliseconds 700;Read-All $c;Wait-Until {(Stats $c).occupiedBytes -gt 0}
 $before=Stats $c;Control $a 'stop.clear';Wait-Until {(Stats $a).occupiedBytes -eq 0}
 if((Stats $c).occupiedBytes -ne $before.occupiedBytes -or (Stats $b).occupiedBytes -eq 0){throw 'Clear crossed game/config ownership'}
 $checks.clearPreservesOtherGameAndOtherSsd=$true
 Control $a 'stop.resize' '4194304';Wait-Until {(Stats $a).capacityBytes -eq 4194304 -and (Stats $b).capacityBytes -eq 4194304}
 if((Stats $c).capacityBytes -ne 8388608){throw 'Resize crossed configurations'}
 Control $a 'stop.resize' '8388608';Wait-Until {(Stats $b).capacityBytes -eq 8388608};$checks.sharedResizeIsIndependent=$true
 Control $a 'stop.pause';Wait-Until {(Stats $a).paused};if((Stats $b).paused){throw 'Pause crossed games'};Control $a 'stop.resume';Wait-Until {-not(Stats $a).paused};$checks.pauseIsIndependent=$true
 # Mutate through the mount: pending fills and resident bytes must not survive.
 $changed=[LoadingRead]::Pattern(88,8388608);[IO.File]::WriteAllBytes((Join-Path $b.source 'asset.bin'),$changed)
 if(-not[LoadingRead]::Equal([LoadingRead]::Read((Join-Path $b.source 'asset.bin'),0,8388608),$changed)){throw 'Stale bytes after write'};$checks.writeInvalidatesPendingAndResidentCopies=$true
 Control $a 'stop';Wait-Until {(Journal $a).restored};if(-not[LoadingRead]::Equal([IO.File]::ReadAllBytes((Join-Path $a.source 'asset.bin')),[LoadingRead]::Pattern(1,8388608))){throw 'Original A changed'}
 if(-not[LoadingRead]::Equal([LoadingRead]::Read((Join-Path $b.source 'asset.bin'),0,8388608),$changed)){throw 'Other mount failed after stop'};$checks.stopRestoresOnlyOwnFolder=$true
 # Stopping keeps validated SSD data: a restart serves it without rereading the HDD.
 Start-App $a;Read-All $a;Wait-Until {(Stats $a).occupiedBytes -gt 0};Control $a 'stop';Wait-Until {(Journal $a).restored}
 Start-App $a;$kept=Stats $a;if($kept.occupiedBytes -le 0){throw 'Stop discarded retained cache data'};Read-All $a;Wait-Until {(Stats $a).ssdReadBytes -gt $kept.ssdReadBytes};$checks.stoppedDataIsRetainedAndReused=$true
 # A file edited while its cache was stopped must never be served stale.
 Control $a 'stop';Wait-Until {(Journal $a).restored};$edited=[LoadingRead]::Pattern(61,8388608);[IO.File]::WriteAllBytes((Join-Path $a.source 'asset.bin'),$edited)
 Start-App $a;if(-not[LoadingRead]::Equal([LoadingRead]::Read((Join-Path $a.source 'asset.bin'),0,8388608),$edited)){throw 'Stale bytes after an offline edit'};$checks.offlineEditIsRevalidated=$true
 Control $a 'stop';Wait-Until {(Journal $a).restored};[IO.File]::WriteAllBytes((Join-Path $a.source 'asset.bin'),[LoadingRead]::Pattern(1,8388608))
 $j=Journal $b;$process=Get-Process -Id $j.workerPid;if($process.StartTime.ToUniversalTime().Ticks -ne $j.workerStartTicks){throw 'Worker identity mismatch'};$process.Kill();Wait-Until {(Journal $b).restored};$checks.workerCrashRestoresOriginal=$true
 $j=Journal $c;$process=Get-Process -Id $j.supervisorPid;if($process.StartTime.ToUniversalTime().Ticks -ne $j.supervisorStartTicks){throw 'Host identity mismatch'};$process.Kill();Wait-Until {-not(Get-Process -Id $j.workerPid -ErrorAction SilentlyContinue)};Start-Sleep -Seconds 1;& $launcher -Action recover -AppId $c.id|Out-Null;Wait-Until {(Journal $c).restored};$checks.hostCrashRecoveryRestoresOriginal=$true
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 foreach($app in $items){try{if(-not $app.started){continue};Wait-Until {Journal $app};$j=Journal $app;if(-not $j.restored){Control $app 'stop';Wait-Until {(Journal $app).restored}};Start-Sleep -Seconds 1;& $launcher -Action remove -AppId $app.id|Out-Null}catch{$proof['cleanup_'+$app.id]=$_.Exception.Message}}
 foreach($pair in @(@('workerSha256','artifacts\loading-cache\folder-worker.exe'),@('sessionSha256','tools\loading-cache\session.ps1'),@('launcherSha256','tools\loading-cache\schedule-session.ps1'))){$proof[$pair[0]]=(Get-FileHash -LiteralPath (Join-Path $workspace $pair[1])).Hash}
 $proof|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\loading-cache\session-proof.json') -Encoding utf8
}
$proof|ConvertTo-Json -Depth 8
if(-not $proof.passed){throw 'Loading-priority integration proof failed'}
