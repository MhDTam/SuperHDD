$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N');$items=@()
$launcher=Join-Path $workspace 'tools\library-cache\schedule-session.ps1'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class LibraryRead {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32")]static extern IntPtr VirtualAlloc(IntPtr a,UIntPtr n,uint t,uint p);
 [DllImport("kernel32")]static extern bool VirtualFree(IntPtr a,UIntPtr n,uint t);
 [DllImport("kernel32",SetLastError=true)]static extern bool ReadFile(SafeFileHandle h,IntPtr b,uint n,out uint done,IntPtr o);
 public static byte[] Read(string p){using(var h=CreateFileW(p,0x80000000,7,IntPtr.Zero,3,0x20000000,IntPtr.Zero)){if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();IntPtr b=VirtualAlloc(IntPtr.Zero,(UIntPtr)65536,0x3000,4);try{uint n;if(!ReadFile(h,b,65536,out n,IntPtr.Zero)||n!=65536)throw new System.IO.IOException("Read failed");byte[] bytes=new byte[n];Marshal.Copy(b,bytes,0,(int)n);return bytes;}finally{VirtualFree(b,UIntPtr.Zero,0x8000);}}}
 public static byte[] Pattern(int seed){byte[] b=new byte[65536];for(int i=0;i<b.Length;i++)b[i]=(byte)((i*17+seed)%251);return b;}
 public static bool Equal(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
}
'@
function Journal($a){Get-Content -LiteralPath (Join-Path $a.runtime 'current.json') -Raw|ConvertFrom-Json}
function Stats($a){$j=Journal $a;Get-Content -LiteralPath (Join-Path $j.task 'status.json') -Raw|ConvertFrom-Json}
function Wait-Until([scriptblock]$test){for($i=0;$i -lt 300;$i++){try{if(& $test){return}}catch{};Start-Sleep -Milliseconds 100};throw 'Library state timeout'}
function Start-App($a){& $launcher -Action start -AppId $a.id -Source $a.source -CapacityBytes 1200029696 -Lab|Out-Null;Wait-Until {(Journal $a).phase -eq 'active'}}
function Control($a,$name,$value='control'){$j=Journal $a;[IO.File]::WriteAllText((Join-Path $j.task $name),$value)}
foreach($seed in @(1,2)){
 $fixture='D:\SuperHDD-SessionLab-'+[Guid]::NewGuid().ToString('N');$source=Join-Path $fixture 'selected'
 $sha=[Security.Cryptography.SHA256]::Create();try{$id=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($source.ToLowerInvariant())))).Replace('-','').ToLowerInvariant().Substring(0,32)}finally{$sha.Dispose()}
 [IO.Directory]::CreateDirectory($source)|Out-Null;[IO.File]::WriteAllBytes((Join-Path $source 'asset.bin'),[LibraryRead]::Pattern($seed))
 $items+=@{id=$id;source=$source;runtime=(Join-Path $workspace ('artifacts\library-runtime\'+$id));seed=$seed}
}
$proof=[ordered]@{runId=$run;passed=$false;checks=@{};error=$null;fixtures=$items}
try{
 $a=$items[0];$b=$items[1];Start-App $a;Start-App $b
 foreach($app in $items){if(-not[LibraryRead]::Equal([LibraryRead]::Read((Join-Path $app.source 'asset.bin')),[LibraryRead]::Pattern($app.seed))){throw 'Wrong data'}}
 $proof.checks.simultaneousSeparateFolderMounts=$true
 Wait-Until {(Stats $a).occupiedBytes -gt 0 -and (Stats $b).occupiedBytes -gt 0}
 Control $a 'stop.clear';Wait-Until {(Stats $a).occupiedBytes -eq 0}
 $before=Stats $b;[void][LibraryRead]::Read((Join-Path $b.source 'asset.bin'));Wait-Until {(Stats $b).hits -gt $before.hits}
 if((Stats $b).sourceReadBytes -ne $before.sourceReadBytes){throw 'Clearing A discarded B'}
 $proof.checks.clearOnePreservesOther=$true
 foreach($size in @(1100021760,1400045568)){Control $b 'stop.resize' ([string]$size);Wait-Until {(Stats $b).capacityBytes -eq $size};$before=Stats $b;[void][LibraryRead]::Read((Join-Path $b.source 'asset.bin'));Wait-Until {(Stats $b).hits -gt $before.hits};if((Stats $b).sourceReadBytes -ne $before.sourceReadBytes){throw 'Resize discarded retained resident slot'}}
 $proof.checks.resizePreservesRetainedData=$true
 Control $a 'stop.pause';Wait-Until {(Stats $a).paused};if((Stats $b).paused){throw 'Pause crossed sessions'};$proof.checks.pauseIsIndependent=$true
 Control $a 'stop';Wait-Until {(Journal $a).restored};if(-not[LibraryRead]::Equal([IO.File]::ReadAllBytes((Join-Path $a.source 'asset.bin')),[LibraryRead]::Pattern(1))){throw 'A source changed'}
 if(-not[LibraryRead]::Equal([LibraryRead]::Read((Join-Path $b.source 'asset.bin')),[LibraryRead]::Pattern(2))){throw 'B failed after stopping A'}
 $proof.checks.stopOnePreservesOther=$true
 $j=Journal $b;$worker=Get-Process -Id $j.workerPid;if($worker.StartTime.ToUniversalTime().Ticks -ne $j.workerStartTicks){throw 'Worker identity mismatch'};$worker.Kill();Wait-Until {(Journal $b).restored}
 $proof.checks.workerCrashAutoRestoresOwnSource=$true
 Start-Sleep -Seconds 1;Start-App $b;$j=Journal $b;$hostProcess=Get-Process -Id $j.supervisorPid;if($hostProcess.StartTime.ToUniversalTime().Ticks -ne $j.supervisorStartTicks){throw 'Host identity mismatch'};$hostProcess.Kill();Wait-Until {-not(Get-Process -Id $j.workerPid -ErrorAction SilentlyContinue)}
 Start-Sleep -Seconds 1;& $launcher -Action recover -AppId $b.id|Out-Null;Wait-Until {(Journal $b).restored}
 $proof.checks.supervisorCrashRecoverRestoresOwnSource=$true
 foreach($app in $items){if(-not[LibraryRead]::Equal([IO.File]::ReadAllBytes((Join-Path $app.source 'asset.bin')),[LibraryRead]::Pattern($app.seed))){throw 'Final source mismatch'}}
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 foreach($app in $items){try{$j=Journal $app;if(-not $j.restored){Control $app 'stop';Wait-Until {(Journal $app).restored}};Start-Sleep -Seconds 1;& $launcher -Action remove -AppId $app.id|Out-Null}catch{$proof['cleanup_'+$app.id]=$_.Exception.Message}}
 foreach($pair in @(@('workerSha256','artifacts\library-cache\folder-worker.exe'),@('sessionSha256','tools\library-cache\session.ps1'),@('launcherSha256','tools\library-cache\schedule-session.ps1'))){$proof[$pair[0]]=(Get-FileHash -LiteralPath (Join-Path $workspace $pair[1])).Hash}
 $proof|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\library-cache\session-proof.json')
}
$proof|ConvertTo-Json -Depth 8
if(-not $proof.passed){throw 'Library sessions proof failed'}
