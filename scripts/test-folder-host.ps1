$ErrorActionPreference='Stop'
$workspace=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$fixture='D:\SuperHDD-SessionLab-'+$run
$source=Join-Path $fixture 'selected'
$proofRoot=Join-Path $workspace ('artifacts\folder-cache\host-'+$run)
$current=Join-Path $workspace 'artifacts\folder-runtime\current.json'
$launcher=Join-Path $workspace 'tools\folder-cache\schedule-session.ps1'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class FolderHostProof {
 [StructLayout(LayoutKind.Sequential)] struct Basic {public long ProcessTime,JobTime;public uint Flags;public UIntPtr Min,Max;public uint Active;public UIntPtr Affinity;public uint Priority,Scheduling;}
 [StructLayout(LayoutKind.Sequential)] struct Io {public ulong A,B,C,D,E,F;}
 [StructLayout(LayoutKind.Sequential)] struct Limits {public Basic Basic;public Io Io;public UIntPtr A,B,C,D;}
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateJobObjectW(IntPtr s,string n);
 [DllImport("kernel32",SetLastError=true)] static extern bool SetInformationJobObject(SafeFileHandle j,int k,IntPtr p,uint n);
 [DllImport("kernel32",SetLastError=true)] static extern bool AssignProcessToJobObject(SafeFileHandle j,IntPtr p);
 [DllImport("kernel32",SetLastError=true)] static extern bool IsProcessInJob(IntPtr p,SafeFileHandle j,out bool inside);
 public static SafeFileHandle Own(IntPtr process){var j=CreateJobObjectW(IntPtr.Zero,null);var l=new Limits();l.Basic.Flags=0x2000;int n=Marshal.SizeOf(l);IntPtr p=Marshal.AllocHGlobal(n);try{Marshal.StructureToPtr(l,p,false);if(j.IsInvalid||!SetInformationJobObject(j,9,p,(uint)n)||!AssignProcessToJobObject(j,process))throw new System.ComponentModel.Win32Exception();return j;}finally{Marshal.FreeHGlobal(p);}}
 public static bool Inside(IntPtr process,SafeFileHandle j){bool inside;if(!IsProcessInJob(process,j,out inside))throw new System.ComponentModel.Win32Exception();return inside;}
}
'@
function State {Get-Content -LiteralPath $current -Raw|ConvertFrom-Json}
function Wait-Phase($phase){for($i=0;$i -lt 300;$i++){try{$s=State;if($s.source -eq $source -and $s.phase -eq $phase){return $s}}catch{};Start-Sleep -Milliseconds 100};throw ('Timed out: '+$phase)}
New-Item -ItemType Directory -Path $source,$proofRoot|Out-Null
[IO.File]::WriteAllText((Join-Path $source 'fixture.txt'),'host-lifetime')
$wrapper=Join-Path $proofRoot 'launch.ps1'
@'
param($launcher,$source)
& $launcher -Action start -Source $source -CapacityBytes 24999952384 -Lab
Start-Sleep -Seconds 300
'@|Set-Content -LiteralPath $wrapper
$proof=[ordered]@{runId=$run;passed=$false;checks=@{};error=$null}
$process=$null;$job=$null
try{
 $psi=[Diagnostics.ProcessStartInfo]::new('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe');$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true
 foreach($arg in @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$wrapper,'-launcher',$launcher,'-source',$source)){$psi.ArgumentList.Add($arg)}
 $process=[Diagnostics.Process]::Start($psi);$job=[FolderHostProof]::Own($process.Handle)
 $s=Wait-Phase 'active';$hostProcess=[Diagnostics.Process]::GetProcessById($s.supervisorPid);$worker=[Diagnostics.Process]::GetProcessById($s.workerPid)
 if([FolderHostProof]::Inside($hostProcess.Handle,$job) -or [FolderHostProof]::Inside($worker.Handle,$job)){throw 'Runtime inherited the launching job'}
 $proof.checks.runtimeOutsideLaunchingJob=$true
 $job.Dispose();$job=$null
 if(-not $process.WaitForExit(5000)){throw 'Launcher did not terminate with its job'}
 Start-Sleep -Seconds 3
 if($hostProcess.HasExited -or $worker.HasExited){throw 'Runtime died with launcher'}
 if([IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'host-lifetime'){throw 'Source unavailable after launcher termination'}
 $proof.checks.sourceAvailableAfterLauncherTermination=$true
 [IO.File]::WriteAllText((Join-Path $source 'fixture.txt'),'durable-after-parent-exit')
 [IO.File]::WriteAllText((Join-Path $s.task 'stop'),'stop')
 $restored=Wait-Phase 'stopped'
 if(-not $restored.restored -or [IO.File]::ReadAllText((Join-Path $source 'fixture.txt')) -ne 'durable-after-parent-exit'){throw 'Restoration failed'}
 $proof.checks.writeAndRestoreAfterLauncherTermination=$true
 $proof.passed=$true
}catch{$proof.error=$_.Exception.Message}finally{
 if($job){$job.Dispose()}
 $s=State
 if($s.source -eq $source -and -not $s.restored){[IO.File]::WriteAllText((Join-Path $s.task 'stop'),'stop');[void](Wait-Phase 'stopped')}
 $proof.launcherSha256=(Get-FileHash -LiteralPath $launcher).Hash
 $proof|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $workspace 'artifacts\folder-cache\host-proof.json')
}
$proof|ConvertTo-Json -Depth 8
if(-not $proof.passed){throw 'Folder host lifetime proof failed'}
