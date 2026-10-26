param([int]$ReadCount=1024,[int]$Rounds=3)
$ErrorActionPreference='Stop'
if($ReadCount -lt 64 -or $ReadCount -gt 4096 -or $Rounds -lt 1 -or $Rounds -gt 5){throw 'Invalid bounded benchmark parameters'}
$workspace=Split-Path -Parent $PSScriptRoot
$j=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\folder-runtime\current.json') -Raw|ConvertFrom-Json
if($j.phase -ne 'active' -or $j.source -ne 'D:\GAMES\DRAGON BALL - Sparking! ZERO'){throw 'Expected active game session'}
$status=Join-Path $j.task 'status.json'
$before=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
if(-not $before.active -or $before.paused){throw 'Cache is not running'}
$run=[DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ')
$output=Join-Path $workspace ('artifacts\benchmarks\folder-reads-'+$run)
New-Item -ItemType Directory -Path $output|Out-Null
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Diagnostics;using System.Runtime.InteropServices;using System.Security.Cryptography;using Microsoft.Win32.SafeHandles;
public sealed class FolderReadResult {public string Mode,Sha256;public int Reads;public long Bytes;public double Seconds,MiBPerSecond,P50Milliseconds,P95Milliseconds;}
public static class FolderReadBenchmark {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)] static extern bool SetFilePointerEx(SafeFileHandle h,long o,IntPtr n,uint m);
 [DllImport("kernel32",SetLastError=true)] static extern bool ReadFile(SafeFileHandle h,IntPtr b,uint n,out uint done,IntPtr overlap);
 [DllImport("kernel32")] static extern IntPtr VirtualAlloc(IntPtr a,UIntPtr n,uint t,uint p);
 [DllImport("kernel32")] static extern bool VirtualFree(IntPtr a,UIntPtr n,uint t);
 static readonly string[] Names={@"SparkingZERO\Content\Paks\pakchunk0-Windows.pak",@"SparkingZERO\Content\Paks\pakchunk0-Windows.ucas",@"SparkingZERO\Content\Paks\pakchunk1-Windows.ucas"};
 static int[] Files;static long[] Offsets;
 public static void Trace(string source,int count){Files=new int[count];Offsets=new long[count];var random=new Random(20261022);var used=new System.Collections.Generic.HashSet<string>();for(int i=0;i<count;i++){int f;long offset;do{f=random.Next(Names.Length);long blocks=new FileInfo(Path.Combine(source,Names[f])).Length/65536;offset=(long)(random.NextDouble()*blocks)*65536;}while(!used.Add(f+":"+offset));Files[i]=f;Offsets[i]=offset;}}
 public static FolderReadResult Run(string root,string mode){var handles=new SafeFileHandle[Names.Length];IntPtr buffer=IntPtr.Zero;try{for(int i=0;i<Names.Length;i++){handles[i]=CreateFileW(Path.Combine(root,Names[i]),0x80000000,7,IntPtr.Zero,3,0x20000000,IntPtr.Zero);if(handles[i].IsInvalid)throw new System.ComponentModel.Win32Exception();}buffer=VirtualAlloc(IntPtr.Zero,(UIntPtr)65536,0x3000,4);if(buffer==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();var data=new byte[65536];var latency=new double[Files.Length];using(var sha=SHA256.Create()){var total=Stopwatch.StartNew();for(int i=0;i<Files.Length;i++){long start=Stopwatch.GetTimestamp();uint done;if(!SetFilePointerEx(handles[Files[i]],Offsets[i],IntPtr.Zero,0)||!ReadFile(handles[Files[i]],buffer,65536,out done,IntPtr.Zero)||done!=65536)throw new IOException("Incomplete benchmark read");latency[i]=(Stopwatch.GetTimestamp()-start)*1000.0/Stopwatch.Frequency;Marshal.Copy(buffer,data,0,65536);sha.TransformBlock(data,0,data.Length,data,0);}sha.TransformFinalBlock(new byte[0],0,0);total.Stop();Array.Sort(latency);long bytes=(long)Files.Length*65536;return new FolderReadResult{Mode=mode,Reads=Files.Length,Bytes=bytes,Seconds=total.Elapsed.TotalSeconds,MiBPerSecond=bytes/1048576.0/total.Elapsed.TotalSeconds,P50Milliseconds=latency[(Files.Length-1)/2],P95Milliseconds=latency[(int)Math.Ceiling(Files.Length*.95)-1],Sha256=BitConverter.ToString(sha.Hash).Replace("-","")};}}
 finally{foreach(var h in handles)if(h!=null)h.Dispose();if(buffer!=IntPtr.Zero)VirtualFree(buffer,UIntPtr.Zero,0x8000);}}
}
'@
$result=[ordered]@{runId=$run;source=$j.source;sessionId=$j.id;blockBytes=65536;readCount=$ReadCount;roundCount=$Rounds;queueDepth=1;seed=20261022;windowsFileCacheBypassed=$true;driveFirmwareCacheNotControlled=$true;concurrentGameActivityPossible=$true;scope='Read-only random game-asset I/O; direct backing versus warm folder proxy; not whole-game loading time';rounds=@();passed=$false;error=$null}
try{
 [FolderReadBenchmark]::Trace($j.backing,$ReadCount)
 $prime=[FolderReadBenchmark]::Run($j.source,'warm-up-not-timed-result')
 $result.warmupSha256=$prime.Sha256
 for($round=0;$round -lt $Rounds;$round++){
  $s0=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
  if($round%2 -eq 0){$direct=[FolderReadBenchmark]::Run($j.backing,'directHdd');$warm=[FolderReadBenchmark]::Run($j.source,'warmFolderCache')}
  else{$warm=[FolderReadBenchmark]::Run($j.source,'warmFolderCache');$direct=[FolderReadBenchmark]::Run($j.backing,'directHdd')}
  if($direct.Sha256 -ne $prime.Sha256 -or $warm.Sha256 -ne $prime.Sha256){throw 'Read stream hashes differ'}
  Start-Sleep -Milliseconds 400
  $s1=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
  $entry=[ordered]@{round=($round+1);order=if($round%2 -eq 0){'direct-then-cache'}else{'cache-then-direct'};direct=$direct;warm=$warm;counterDelta=@{hits=($s1.hits-$s0.hits);misses=($s1.misses-$s0.misses);ssdReadBytes=($s1.ssdReadBytes-$s0.ssdReadBytes);sourceReadBytes=($s1.sourceReadBytes-$s0.sourceReadBytes)}}
  $result.rounds+=$entry
  $entry|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $output ('round-'+($round+1)+'.json'))
 }
 $after=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
 if($after.checksumFailures -ne $before.checksumFailures -or $after.cacheErrors -ne $before.cacheErrors){throw 'Cache error counters increased'}
 $result.checksumFailuresAdded=0;$result.cacheErrorsAdded=0;$result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
 $result|ConvertTo-Json -Depth 10|Set-Content -LiteralPath (Join-Path $output 'result.json')
 [IO.File]::WriteAllText((Join-Path $workspace 'artifacts\benchmarks\latest-folder-read-run.txt'),$output)
}
$result|ConvertTo-Json -Depth 10
if(-not $result.passed){throw 'Read-only folder benchmark failed'}
