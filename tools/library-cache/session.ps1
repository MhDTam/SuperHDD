param([ValidateSet('start','recover')][string]$Action='start',[Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{32}$')][string]$AppId,[string]$Source,[UInt64]$CapacityBytes=24999952384,[switch]$Lab)
$ErrorActionPreference='Stop'
if($PSVersionTable.PSVersion.Major -le 5){$env:PSModulePath=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'}
$workspace=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$runtime=Join-Path $workspace ('artifacts\library-runtime\'+$AppId)
$current=Join-Path $runtime 'current.json'
$worker=Join-Path $workspace 'artifacts\library-cache\folder-worker.exe'
New-Item -ItemType Directory -Path $runtime -Force|Out-Null
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class FolderSessionNative {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string p,uint a,uint s,IntPtr d,uint c,uint f,IntPtr t);
 [DllImport("kernel32",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint c,IntPtr i,uint n,byte[] o,uint l,out uint done,IntPtr x);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool MoveFileExW(string a,string b,uint f);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool RemoveDirectoryW(string p);
 [StructLayout(LayoutKind.Sequential)] struct FileInfo {public uint Attributes,CreationLow,CreationHigh,AccessLow,AccessHigh,WriteLow,WriteHigh,Volume,SizeHigh,SizeLow,Links,IndexHigh,IndexLow;}
 [DllImport("kernel32",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle h,out FileInfo i);
 [StructLayout(LayoutKind.Sequential)] struct BasicLimit {public long ProcessTime,JobTime;public uint Flags;public UIntPtr MinWorking,MaxWorking;public uint ActiveProcesses;public UIntPtr Affinity;public uint Priority,Scheduling;}
 [StructLayout(LayoutKind.Sequential)] struct IoCounters {public ulong ReadOps,WriteOps,OtherOps,ReadBytes,WriteBytes,OtherBytes;}
 [StructLayout(LayoutKind.Sequential)] struct ExtendedLimit {public BasicLimit Basic;public IoCounters Io;public UIntPtr ProcessMemory,JobMemory,PeakProcess,PeakJob;}
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateJobObjectW(IntPtr security,string name);
 [DllImport("kernel32",SetLastError=true)] static extern bool SetInformationJobObject(SafeFileHandle job,int kind,IntPtr info,uint size);
 [DllImport("kernel32",SetLastError=true)] static extern bool AssignProcessToJobObject(SafeFileHandle job,IntPtr process);
 public static SafeFileHandle ChildJob(IntPtr process){var job=CreateJobObjectW(IntPtr.Zero,null);var info=new ExtendedLimit();info.Basic.Flags=0x2000;int size=Marshal.SizeOf(info);IntPtr p=Marshal.AllocHGlobal(size);try{Marshal.StructureToPtr(info,p,false);if(job.IsInvalid||!SetInformationJobObject(job,9,p,(uint)size)||!AssignProcessToJobObject(job,process)){job.Dispose();throw new System.ComponentModel.Win32Exception();}return job;}finally{Marshal.FreeHGlobal(p);}}
 public static string Identity(string p){using(var h=CreateFileW(p,0x80,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){FileInfo i;if(h.IsInvalid||!GetFileInformationByHandle(h,out i))throw new System.ComponentModel.Win32Exception();if((i.Attributes&0x400)!=0)throw new IOException("Reparse source");return i.Volume.ToString("X8")+":"+i.IndexHigh.ToString("X8")+i.IndexLow.ToString("X8");}}
 public static string Reparse(string p){using(var h=CreateFileW(p,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){byte[] b=new byte[16384];uint n;if(h.IsInvalid||!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out n,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();if(BitConverter.ToUInt32(b,0)!=0xa0000003)throw new IOException("Unexpected mount tag");Array.Resize(ref b,(int)n);return Convert.ToBase64String(b);}}
 public static void Move(string a,string b,bool replace){if(!MoveFileExW(a,b,replace?9u:8u))throw new System.ComponentModel.Win32Exception();}
 public static void RemoveMount(string p,string tag){if(Reparse(p)!=tag)throw new IOException("Mount identity changed");if(!RemoveDirectoryW(p))throw new System.ComponentModel.Win32Exception();}
}
'@
function Save {
    $bytes=[Text.Encoding]::UTF8.GetBytes(($script:record|ConvertTo-Json -Depth 8));$tmp=$current+'.tmp'
    $file=[IO.File]::Open($tmp,[IO.FileMode]::Create,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$file.Write($bytes,0,$bytes.Length);$file.Flush($true)}finally{$file.Dispose()}
    [FolderSessionNative]::Move($tmp,$current,$true)
}
function Validate-Record {
    if($script:record.id -notmatch '^[a-f0-9]{32}$'){throw 'Invalid session ID'}
    $expectedTask=Join-Path $runtime $script:record.id
    if($script:record.task -ne $expectedTask -or $script:record.source -notmatch '^D:\\[^:]+$' -or $script:record.source -match '[\r\n"<>|?*]'){throw 'Invalid session paths'}
    $parent=Split-Path -Parent $script:record.source
    if($script:record.backing -ne (Join-Path $parent ('.SuperHDD-'+$script:record.id))){throw 'Invalid backing path'}
}
function Restore {
    Validate-Record
    $src=$script:record.source;$back=$script:record.backing
    if(Test-Path -LiteralPath $back){
        if([FolderSessionNative]::Identity($back) -ne $script:record.sourceIdentity){throw 'Source identity changed; preserve both paths'}
        if(Test-Path -LiteralPath $src){
            if((Get-Item -LiteralPath $src -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){
                if(-not $script:record.mountTag){throw 'Missing mount identity; preserve backing'}
                [FolderSessionNative]::RemoveMount($src,$script:record.mountTag)
            }else{
                if(@(Get-ChildItem -LiteralPath $src -Force).Count){throw 'Restore target contains files; preserve both paths'}
                [IO.Directory]::Delete($src,$false)
            }
        }
        [FolderSessionNative]::Move($back,$src,$false)
    }
    if([FolderSessionNative]::Identity($src) -ne $script:record.sourceIdentity){throw 'Restored source identity mismatch'}
    $cache=Join-Path $script:record.task 'cache.bin'
    if(Test-Path -LiteralPath $cache){Remove-Item -LiteralPath $cache}
    $script:record.phase='stopped';$script:record.restored=$true;Save
}
function Same-Process($id,$ticks){
    try{$p=Get-Process -Id $id -ErrorAction Stop;return $p.StartTime.ToUniversalTime().Ticks -eq $ticks}catch{return $false}
}
$sessionLock=[IO.File]::Open((Join-Path $runtime 'session.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$process=$null;$record=$null;$errors=$null;$ownsRecovery=$false;$job=$null
try {
    if(Test-Path -LiteralPath $current){
        $record=Get-Content -LiteralPath $current -Raw|ConvertFrom-Json
        Validate-Record
        if(Same-Process $record.supervisorPid $record.supervisorStartTicks){throw 'A folder session is already running'}
        if($record.workerPid -and (Same-Process $record.workerPid $record.workerStartTicks)){throw 'A recorded worker remains alive; use Stop first'}
        $ownsRecovery=$true
        Restore
    }
    if($Action -eq 'recover'){return}
    $Source=[IO.Path]::GetFullPath($Source).TrimEnd('\')
    if($Source -notmatch '^D:\\[^:]+$' -or $Source -match '[\r\n"<>|?*]' -or -not(Test-Path -LiteralPath $Source -PathType Container)){throw 'Choose an existing D subfolder'}
    if($Lab -and $Source -notmatch '^D:\\SuperHDD-SessionLab-[a-f0-9]{32}\\selected$'){throw 'Lab mode requires a new session fixture'}
    if($CapacityBytes -lt 1GB -or $CapacityBytes -gt 25000000000){throw 'Invalid folder cache capacity'}
    $ancestor=$Source
    while($ancestor.Length -gt 3){if((Get-Item -LiteralPath $ancestor -Force).Attributes -band ([IO.FileAttributes]::ReparsePoint -bor [IO.FileAttributes]::Encrypted)){throw 'Reparse or EFS-encrypted source ancestry is unsupported'};$ancestor=Split-Path -Parent $ancestor}
    if(@(Get-Process -ErrorAction SilentlyContinue|Where-Object {$_.Path -and $_.Path.StartsWith($Source+'\',[StringComparison]::OrdinalIgnoreCase)}).Count){throw 'Close the selected game before starting its cache'}
    $pending=New-Object 'System.Collections.Generic.Stack[string]';$pending.Push($Source)
    while($pending.Count){foreach($path in [IO.Directory]::EnumerateFileSystemEntries($pending.Pop())){$attr=[IO.File]::GetAttributes($path);if($attr -band ([IO.FileAttributes]::ReparsePoint -bor [IO.FileAttributes]::Encrypted)){throw 'Selected folder contains reparse points or EFS-encrypted data'};if($attr -band [IO.FileAttributes]::Directory){$pending.Push($path)}}}
    foreach($path in @('C:\Windows\System32\drivers\dokan2.sys','C:\Windows\System32\dokan2.dll')){if((Get-AuthenticodeSignature -LiteralPath $path).Status -ne 'Valid'){throw 'Installed Dokan signature is invalid'}}
    $qualified=Get-Content -LiteralPath (Join-Path $workspace 'artifacts\library-cache\qualified.json') -Raw|ConvertFrom-Json
    if(-not $Lab -and -not $qualified.verified){throw 'Folder runtime has not completed integration qualification'}
    if((Get-FileHash -LiteralPath $worker).Hash -ne $qualified.workerSha256 -or (Get-FileHash -LiteralPath $PSCommandPath).Hash -ne $qualified.sessionSha256){throw 'Folder runtime qualification mismatch'}
    if((Get-Volume -DriveLetter M).SizeRemaining -lt ($CapacityBytes+1GB)){throw 'Insufficient SSD space'}
    $id=[Guid]::NewGuid().ToString('N');$task=Join-Path $runtime $id;New-Item -ItemType Directory -Path $task|Out-Null
    $backing=Join-Path (Split-Path -Parent $Source) ('.SuperHDD-'+$id)
    $self=Get-Process -Id $PID
    $record=[ordered]@{id=$id;source=$Source;backing=$backing;task=$task;sourceIdentity=[FolderSessionNative]::Identity($Source);phase='prepared';supervisorPid=$PID;supervisorStartTicks=$self.StartTime.ToUniversalTime().Ticks;workerPid=$null;workerStartTicks=$null;mountTag=$null;restored=$false;error=$null;capacityBytes=$CapacityBytes}
    $ownsRecovery=$true
    Save
    [FolderSessionNative]::Move($Source,$backing,$false);$record.phase='sourceMoved';Save
    [IO.Directory]::CreateDirectory($Source)|Out-Null
    $ready=Join-Path $task 'ready';$stop=Join-Path $task 'stop';$status=Join-Path $task 'status.json';$cache=Join-Path $task 'cache.bin'
    $psi=New-Object Diagnostics.ProcessStartInfo;$psi.FileName=$worker;$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true
    $arguments=@('run',$backing,$Source,$cache,[string]$CapacityBytes,$ready,$stop,$status)
    $psi.Arguments=($arguments|ForEach-Object {if($_.Contains('"')){throw 'Invalid argument'};'"'+$_+'"'}) -join ' '
    $process=[Diagnostics.Process]::Start($psi);$job=[FolderSessionNative]::ChildJob($process.Handle);$errors=$process.StandardError.ReadToEndAsync();$record.workerPid=$process.Id;$record.workerStartTicks=$process.StartTime.ToUniversalTime().Ticks;Save
    for($i=0;$i -lt 200;$i++){
        if($process.HasExited){throw ('Worker exited before ready: '+$errors.GetAwaiter().GetResult())}
        if(Test-Path -LiteralPath $ready){$record.mountTag=[FolderSessionNative]::Reparse($Source);$record.phase='active';Save;break}
        Start-Sleep -Milliseconds 100
    }
    if($record.phase -ne 'active'){throw 'Folder worker startup timed out'}
    while(-not $process.WaitForExit(500)){}
    $errors.GetAwaiter().GetResult()|Set-Content -LiteralPath (Join-Path $task 'worker.log')
    if($process.ExitCode -ne 0){$record.error='Worker exited: '+$process.ExitCode}
}catch {
    if($record -and $ownsRecovery){$record.error=$_.Exception.Message;try{Save}catch{}}
    Write-Error $_ -ErrorAction Continue
}finally {
    if($process -and -not $process.HasExited){[IO.File]::WriteAllText((Join-Path $record.task 'stop'),'stop');if(-not $process.WaitForExit(30000)){$process.Kill();$process.WaitForExit()}}
    if($record -and $ownsRecovery){try{Restore}catch{$record.phase='recoveryRequired';$record.error=$_.Exception.Message;Save}}
    if($job){$job.Dispose()}
    $sessionLock.Dispose()
}
