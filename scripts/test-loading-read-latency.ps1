param([string]$Candidate='artifacts\loading-cache\parallel-candidate\folder-worker.exe',[string]$Baseline='artifacts\loading-cache\folder-worker.exe',[string]$ProbeBinary='artifacts\loading-cache\parallel-candidate\read-load.exe')
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class LatencyMount {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)]static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint n,byte[] o,uint l,out uint done,IntPtr x);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool RemoveDirectoryW(string p);
 public static string Tag(string p){using(var h=CreateFileW(p,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){byte[] b=new byte[16384];uint n;if(h.IsInvalid||!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();if(BitConverter.ToUInt32(b,0)!=0xa0000003)throw new Exception("Unexpected mount tag");return Convert.ToBase64String(b,0,(int)n);}}
 public static void Remove(string p,string expected){if(Tag(p)!=expected)throw new Exception("Mount identity changed");if(!RemoveDirectoryW(p))throw new System.ComponentModel.Win32Exception();}
}
'@
$workspace=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$output=Join-Path $workspace ('artifacts\loading-cache\latency-'+$run)
$fixture='D:\SuperHDD-LatencyLab-'+$run
$backing=Join-Path $fixture 'backing';$mount=Join-Path $fixture 'selected'
$probe=Join-Path $workspace $ProbeBinary
$old=Join-Path $workspace $Baseline
$new=Join-Path $workspace $Candidate
if((Get-FileHash -LiteralPath $old).Hash -eq (Get-FileHash -LiteralPath $new).Hash){throw 'Baseline and candidate are identical; provide a distinct baseline binary'}
$proof=[ordered]@{runId=$run;passed=$false;scope='Synthetic first-pass parallel reads of one archive, buffered HDD source warmed in the Windows cache; not game frame times or physical-HDD throughput';oldSha256=(Get-FileHash -LiteralPath $old).Hash;candidateSha256=(Get-FileHash -LiteralPath $new).Hash;rounds=@();error=$null;fixture=$fixture}
$process=$null
[IO.Directory]::CreateDirectory($output)|Out-Null
[IO.Directory]::CreateDirectory($backing)|Out-Null
[IO.Directory]::CreateDirectory($mount)|Out-Null
$asset=Join-Path $backing 'archive.bin'
& $probe make $asset;if($LASTEXITCODE){throw 'Fixture creation failed'}
$sourceHash=(Get-FileHash -LiteralPath $asset).Hash
try{
 foreach($iteration in 1..3){foreach($bytes in @(4096,65536)){foreach($engine in @('previous','candidate')){
  $name=$engine+'-'+$bytes+'-'+$iteration;$dir=Join-Path $output $name;[IO.Directory]::CreateDirectory($dir)|Out-Null
  $ready=Join-Path $dir 'ready';$stop=Join-Path $dir 'stop';$status=Join-Path $dir 'status.json';$cache=Join-Path $dir 'cache.bin'
  & $probe warm $asset;if($LASTEXITCODE){throw 'Source warmup failed'}
  $worker=if($engine -eq 'previous'){$old}else{$new}
  $psi=[Diagnostics.ProcessStartInfo]::new($worker);$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true
  foreach($arg in @('run',$backing,$mount,$cache,'33554432',$ready,$stop,$status,('a'*32))){$psi.ArgumentList.Add($arg)}
  $process=[Diagnostics.Process]::Start($psi);$stderr=$process.StandardError.ReadToEndAsync()
  for($i=0;$i -lt 200;$i++){if($process.HasExited){throw ('Worker exited: '+$stderr.GetAwaiter().GetResult())};if(Test-Path -LiteralPath $ready){break};Start-Sleep -Milliseconds 50}
  if(-not(Test-Path -LiteralPath $ready)){throw 'Mount startup timed out'}
  $mountTag=[LatencyMount]::Tag($mount)
  $text=& $probe run (Join-Path $mount 'archive.bin') $bytes 8
  if($LASTEXITCODE){throw ('Concurrent read validation failed '+$text)}
  $measurement=$text|ConvertFrom-Json
  Start-Sleep -Milliseconds 650
  $telemetry=Get-Content -LiteralPath $status -Raw|ConvertFrom-Json
  if($telemetry.cacheErrors -or $telemetry.checksumFailures){throw 'Cache error during workload'}
  if($telemetry.sourceReadBytes -gt $telemetry.requestedReadBytes){throw 'Worker amplified source reads'}
  $proof.rounds+=@{engine=$engine;iteration=$iteration;measurement=$measurement;telemetry=$telemetry}
  [IO.File]::WriteAllText($stop,'stop');if(-not $process.WaitForExit(30000)){throw 'Fixture worker did not stop'}
  if($process.ExitCode -ne 0){throw ('Worker exit '+$process.ExitCode+' '+$stderr.GetAwaiter().GetResult())}
  if((Get-Item -LiteralPath $mount -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){[LatencyMount]::Remove($mount,$mountTag);[IO.Directory]::CreateDirectory($mount)|Out-Null}
  $process=$null
 }}}
 if((Get-FileHash -LiteralPath $asset).Hash -ne $sourceHash){throw 'Fixture original was changed'}
 $parallel=@($proof.rounds|Where-Object {$_.engine -eq 'candidate' -and $_.telemetry.sourceParallelMax -gt 1})
 if(-not $parallel.Count){throw 'Candidate did not demonstrate concurrent source I/O'}
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 if($process -and -not $process.HasExited){[IO.File]::WriteAllText($stop,'stop');if(-not $process.WaitForExit(30000)){$proof.error+='; preserve mounted fixture'}}
 $proof|ConvertTo-Json -Depth 12|Set-Content -LiteralPath (Join-Path $output 'result.json') -Encoding utf8
}
$proof.rounds|ForEach-Object {[pscustomobject]@{engine=$_.engine;iteration=$_.iteration;requestBytes=$_.measurement.requestBytes;p95Ms=$_.measurement.p95Ms;maxMs=$_.measurement.maxMs;MiBPerSecond=$_.measurement.MiBPerSecond;sourceParallelMax=$_.telemetry.sourceParallelMax}}|Format-Table -AutoSize
[pscustomobject]@{passed=$proof.passed;error=$proof.error;result=(Join-Path $output 'result.json')}|ConvertTo-Json
if(-not $proof.passed){throw 'Read latency fixture failed'}
