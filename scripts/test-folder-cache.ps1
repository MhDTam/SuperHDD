param([UInt64]$CapacityBytes=25000000000,[switch]$CrashRecovery,[string]$WorkerBinary='artifacts\folder-cache\folder-worker.exe',[ValidateSet('folder-cache','library-cache','config-cache')][string]$EvidenceGroup='folder-cache')
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $root ('artifacts\'+$EvidenceGroup+'\runs\'+$run)
$testRoot='D:\SuperHDD-FolderLab-'+$run
$selected=Join-Path $testRoot 'selected'
$backing=Join-Path $testRoot 'backing'
$cache=Join-Path $runRoot 'cache.bin'
$ready=Join-Path $runRoot 'ready'
$stop=Join-Path $runRoot 'stop'
$status=Join-Path $runRoot 'status.json'
$journal=Join-Path $runRoot 'journal.json'
$worker=Join-Path $root $WorkerBinary
if($CapacityBytes -lt 65536 -or $CapacityBytes -gt 25000000000){throw 'Invalid cache budget'}
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class FolderFixture {
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32",SetLastError=true)] static extern bool SetFilePointerEx(SafeFileHandle h,long offset,IntPtr next,uint origin);
 [DllImport("kernel32",SetLastError=true)] static extern bool ReadFile(SafeFileHandle h,IntPtr buffer,uint length,out uint count,IntPtr overlapped);
 [DllImport("kernel32",SetLastError=true)] static extern IntPtr VirtualAlloc(IntPtr address,UIntPtr size,uint kind,uint protection);
 [DllImport("kernel32")] static extern bool VirtualFree(IntPtr address,UIntPtr size,uint kind);
 [DllImport("kernel32",SetLastError=true)] static extern bool DeviceIoControl(SafeFileHandle h,uint control,IntPtr input,uint inputLength,byte[] output,uint outputLength,out uint returned,IntPtr overlap);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool RemoveDirectoryW(string path);
 public static string Reparse(string path){using(var h=CreateFileW(path,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)){if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();byte[] b=new byte[16384];uint length;if(!DeviceIoControl(h,0x900a8,IntPtr.Zero,0,b,(uint)b.Length,out length,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();if(BitConverter.ToUInt32(b,0)!=0xa0000003)throw new IOException("Not a mount-point tag");Array.Resize(ref b,(int)length);return Convert.ToBase64String(b);}}
 public static void RemoveOwnedMount(string path,string expected){if(Reparse(path)!=expected)throw new IOException("Mount tag identity changed");if(!RemoveDirectoryW(path))throw new System.ComponentModel.Win32Exception();}
 public static byte[] Pattern(int length,int seed){byte[] b=new byte[length];for(int i=0;i<length;i++)b[i]=(byte)((i*17+i/11+seed*31)%251);return b;}
 public static void MappedWrite(string path,long offset,byte value){using(var map=System.IO.MemoryMappedFiles.MemoryMappedFile.CreateFromFile(path,FileMode.Open,null,0,System.IO.MemoryMappedFiles.MemoryMappedFileAccess.ReadWrite))using(var view=map.CreateViewAccessor()){view.Write(offset,value);view.Flush();}}
 public static byte[] DirectRead(string path,long offset,int count){
  using(var h=CreateFileW(path,0x80000000,7,IntPtr.Zero,3,0x20000000,IntPtr.Zero)){
   if(h.IsInvalid)throw new System.ComponentModel.Win32Exception();
   IntPtr p=VirtualAlloc(IntPtr.Zero,(UIntPtr)count,0x3000,4);if(p==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
   try{uint done;if(!SetFilePointerEx(h,offset,IntPtr.Zero,0)||!ReadFile(h,p,(uint)count,out done,IntPtr.Zero))throw new System.ComponentModel.Win32Exception();byte[] b=new byte[done];Marshal.Copy(p,b,0,(int)done);return b;}finally{VirtualFree(p,UIntPtr.Zero,0x8000);}
  }
 }
 public static bool Equal(byte[] a,byte[] b){if(a.Length!=b.Length)return false;for(int i=0;i<a.Length;i++)if(a[i]!=b[i])return false;return true;}
}
'@
function Check($condition,[string]$message){if(-not $condition){throw $message}}
function Stats {for($i=0;$i -lt 25;$i++){try{return Get-Content -LiteralPath $status -Raw | ConvertFrom-Json}catch{Start-Sleep -Milliseconds 100}};throw 'No worker telemetry'}
function Wait-Stats([scriptblock]$predicate){for($i=0;$i -lt 50;$i++){$s=Stats;if(& $predicate $s){return $s};Start-Sleep -Milliseconds 100};throw 'Worker telemetry did not reach expected state'}
function Save-Journal([string]$phase){
    $value=@{runId=$run;phase=$phase;selected=$selected;backing=$backing;testRoot=$testRoot;mountReparse=$script:mountReparse;workerPid=if($process){$process.Id}else{$null}}
    $bytes=[Text.Encoding]::UTF8.GetBytes(($value|ConvertTo-Json));$tmp=$journal+'.tmp'
    $f=[IO.File]::Open($tmp,[IO.FileMode]::Create,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$f.Write($bytes);$f.Flush($true)}finally{$f.Dispose()}
    [IO.File]::Move($tmp,$journal,$true)
}
function Recover {
    # Paths derive from this new run, never from an arbitrary journal field.
    if([IO.Path]::GetFullPath($testRoot) -ne ('D:\SuperHDD-FolderLab-'+$run)){throw 'Unexpected recovery root'}
    foreach($path in @($testRoot,$backing)){if(Test-Path -LiteralPath $path){Check (-not((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Recovery root became a reparse point'}}
    if(Test-Path -LiteralPath $backing){
        if(Test-Path -LiteralPath $selected){
            if((Get-Item -LiteralPath $selected).Attributes -band [IO.FileAttributes]::ReparsePoint){
                Check ([bool]$script:mountReparse) 'Missing mount identity; preserve backing'
                [FolderFixture]::RemoveOwnedMount($selected,$script:mountReparse)
            }else{
                Check (@(Get-ChildItem -LiteralPath $selected -Force).Count -eq 0) 'Recovery target contains files; preserve both'
                [IO.Directory]::Delete($selected,$false)
            }
        }
        [IO.Directory]::Move($backing,$selected)
    }
    Save-Journal 'restored'
}
function Start-Worker {
    foreach($path in @($ready,$stop,$cache)){if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}}
    Save-Journal 'prepared'
    [IO.Directory]::Move($selected,$backing)
    Save-Journal 'sourceMoved'
    [IO.Directory]::CreateDirectory($selected)|Out-Null
    $psi=[Diagnostics.ProcessStartInfo]::new($worker)
    $psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardError=$true;$psi.RedirectStandardOutput=$true
    foreach($arg in @('run',$backing,$selected,$cache,[string]$CapacityBytes,$ready,$stop,$status)){$psi.ArgumentList.Add($arg)}
    $script:process=[Diagnostics.Process]::Start($psi)
    $script:errors=$process.StandardError.ReadToEndAsync();$script:stdout=$process.StandardOutput.ReadToEndAsync()
    Save-Journal 'workerStarted'
    for($i=0;$i -lt 200;$i++){
        if($process.HasExited){throw ('Worker exited before ready: '+$errors.GetAwaiter().GetResult())}
        if(Test-Path -LiteralPath $ready){$script:mountReparse=[FolderFixture]::Reparse($selected);Save-Journal 'mounted';return}
        Start-Sleep -Milliseconds 100
    }
    throw 'Worker startup timed out'
}
$process=$null;$errors=$null;$stdout=$null
$result=[ordered]@{runId=$run;passed=$false;scope='New selected-folder read/write cache fixture only';capacityRequested=$CapacityBytes;existingUserFoldersChanged=$false;checks=@{};error=$null}
$bootBefore=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$driver='C:\Windows\System32\drivers\dokan2.sys';$dll='C:\Windows\System32\dokan2.dll'
foreach($path in @($driver,$dll)){Check ((Get-AuthenticodeSignature -LiteralPath $path).Status -eq 'Valid') 'Existing Dokan signature check failed'}
Check ((Get-AuthenticodeSignature -LiteralPath $driver).SignerCertificate.Subject -like 'CN=Microsoft Windows Hardware Compatibility Publisher,*') 'Expected Microsoft-signed Dokan driver'
$dDisk=Get-Partition -DriveLetter D|Get-Disk;$mDisk=Get-Partition -DriveLetter M|Get-Disk
Check ($dDisk.FriendlyName -match 'HUA722020ALA331' -and $mDisk.FriendlyName -match '980 PRO' -and $dDisk.UniqueId -ne $mDisk.UniqueId) 'Expected D/M physical devices'
Check ((Get-Volume -DriveLetter M).SizeRemaining -gt ($CapacityBytes+1GB)) 'Insufficient workspace SSD space'
New-Item -ItemType Directory -Path $runRoot|Out-Null
Check (-not(Test-Path -LiteralPath $testRoot)) 'Test root must be new'
[IO.Directory]::CreateDirectory($selected)|Out-Null
$alpha=[FolderFixture]::Pattern(524288,1);$beta=[FolderFixture]::Pattern(65869,2)
[IO.File]::WriteAllBytes((Join-Path $selected 'alpha.bin'),$alpha)
[IO.File]::WriteAllBytes((Join-Path $selected 'beta.bin'),$beta)
try {
    Start-Worker
    $a=Join-Path $selected 'alpha.bin';$b=Join-Path $selected 'beta.bin'
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($a,0,65536),$alpha[0..65535])) 'Cold alpha bytes mismatch'
    $cold=Wait-Stats {param($s)$s.misses -gt 0}
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($a,0,65536),$alpha[0..65535])) 'Warm alpha bytes mismatch'
    $warm=Wait-Stats {param($s)$s.hits -gt $cold.hits}
    Check ($warm.sourceReadBytes -eq $cold.sourceReadBytes -and $warm.ssdReadBytes -gt $cold.ssdReadBytes) 'Warm read did not use SSD exclusively'
    $result.checks.warmSsdRead=$true
    [IO.File]::WriteAllText($stop+'.pause','pause')
    $paused=Wait-Stats {param($s)$s.paused -eq $true}
    [void][FolderFixture]::DirectRead($a,0,65536)
    $bypassed=Wait-Stats {param($s)$s.sourceReadBytes -gt $paused.sourceReadBytes}
    Check ($bypassed.ssdReadBytes -eq $paused.ssdReadBytes) 'Pause still served SSD data'
    [IO.File]::WriteAllText($stop+'.resume','resume')
    [void](Wait-Stats {param($s)$s.paused -eq $false})
    [void][FolderFixture]::DirectRead($a,0,65536)
    $result.checks.pauseResume=$true
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($b,65536,65536),$beta[65536..65868])) 'Odd EOF bytes mismatch'
    $result.checks.multipleFilesAndOddEof=$true
    $outsideWriteBlocked=$false
    try{$f=[IO.File]::Open((Join-Path $backing 'alpha.bin'),[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite);$f.Dispose()}catch{$outsideWriteBlocked=$true}
    Check $outsideWriteBlocked 'Cached source permits an unobserved external writer'
    $result.checks.externalWriterExcluded=$true
    $patch=[FolderFixture]::Pattern(713,9)
    $f=[IO.File]::Open($a,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)
    try{$f.Position=123;$f.Write($patch);$f.Flush($true)}finally{$f.Dispose()}
    [Array]::Copy($patch,0,$alpha,123,$patch.Length)
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($a,0,65536),$alpha[0..65535])) 'Read returned stale data after write'
    Check ([FolderFixture]::Equal([IO.File]::ReadAllBytes((Join-Path $backing 'alpha.bin')),$alpha)) 'Acknowledged write not present in HDD source'
    $result.checks.durableUnalignedWriteAndInvalidation=$true
    $new=Join-Path $selected 'new.bin';[IO.File]::WriteAllBytes($new,[byte[]](1,2,3,4,5))
    [IO.File]::Move($new,(Join-Path $selected 'renamed.bin'))
    $f=[IO.File]::Open((Join-Path $selected 'renamed.bin'),[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite)
    try{$f.SetLength(3);$f.Flush($true)}finally{$f.Dispose()}
    Check ([FolderFixture]::Equal([IO.File]::ReadAllBytes((Join-Path $selected 'renamed.bin')),[byte[]](1,2,3))) 'Truncate/rename mismatch'
    [IO.File]::Delete((Join-Path $selected 'renamed.bin'))
    Check (-not(Test-Path -LiteralPath (Join-Path $selected 'renamed.bin'))) 'Delete failed'
    $result.checks.createRenameTruncateDelete=$true
    $sub=Join-Path $selected 'sub';[IO.Directory]::CreateDirectory($sub)|Out-Null
    [IO.File]::WriteAllBytes((Join-Path $sub 'nested.bin'),[byte[]](7,8,9))
    Check (@(Get-ChildItem -LiteralPath $sub).Count -eq 1) 'Directory listing failed'
    [IO.File]::Delete((Join-Path $sub 'nested.bin'));[IO.Directory]::Delete($sub)
    $result.checks.nestedDirectoryCreateListDelete=$true
    [FolderFixture]::MappedWrite($a,901,211)
    $alpha[901]=211
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($a,0,65536),$alpha[0..65535])) 'Mapped write was stale'
    $result.checks.mappedWriteCoherence=$true
    [IO.File]::WriteAllBytes($a+':SuperHDD-test',[byte[]](19,23,29))
    Check ([FolderFixture]::Equal([IO.File]::ReadAllBytes($a+':SuperHDD-test'),[byte[]](19,23,29))) 'Named stream data mismatch'
    Check ([FolderFixture]::Equal([FolderFixture]::DirectRead($a,0,65536),$alpha[0..65535])) 'Named stream aliased the main cache'
    $result.checks.namedStreamIsolation=$true
    $inventory=Wait-Stats {param($s)@($s.cachedFiles).Count -gt 0}
    Check ($inventory.cachedFiles[0].path.StartsWith($selected+'\',[StringComparison]::OrdinalIgnoreCase)) 'Inventory exposes an incorrect path'
    Check ($inventory.leasedFileCount -le 512) 'Unbounded source leases'
    Check (($inventory.cachedFiles|Measure-Object -Property residentBytes -Sum).Sum -eq $inventory.occupiedBytes) 'Resident inventory sum differs from cache occupancy'
    $result.checks.liveInventory=$true
    [IO.File]::WriteAllText($stop+'.clear','clear')
    $cleared=Wait-Stats {param($s)$s.occupiedBytes -eq 0}
    $result.checks.clear=$true
    $result.telemetry=Stats
    [IO.File]::WriteAllText($stop,'stop')
    Check ($process.WaitForExit(30000)) 'Worker did not stop'
    Check ($process.ExitCode -eq 0) ('Worker exit '+$process.ExitCode)
    $result.workerStderr=$errors.GetAwaiter().GetResult()
    Recover
    Check ([FolderFixture]::Equal([IO.File]::ReadAllBytes((Join-Path $selected 'alpha.bin')),$alpha)) 'Restored source mismatch'
    $result.checks.normalRestore=$true
    if($CrashRecovery){
        Start-Worker
        [void][FolderFixture]::DirectRead((Join-Path $selected 'alpha.bin'),0,65536)
        $process.Kill();Check ($process.WaitForExit(10000)) 'Owned worker did not terminate'
        Recover
        Check ([FolderFixture]::Equal([IO.File]::ReadAllBytes((Join-Path $selected 'alpha.bin')),$alpha)) 'Crash recovery source mismatch'
        $result.checks.processCrashRestore=$true
    }
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    if($process -and -not $process.HasExited){[IO.File]::WriteAllText($stop,'stop');if(-not $process.WaitForExit(30000)){$result.error+='; worker still running; preserve source'}}
    if(-not $process -or $process.HasExited){
        if($errors){$result.workerStderr=$errors.GetAwaiter().GetResult()}
        try{Recover}catch{$result.passed=$false;$result.recoveryError=$_.Exception.Message}
        if(Test-Path -LiteralPath $cache){Remove-Item -LiteralPath $cache}
    }
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootUnchanged=($bootBefore.secureBootEnabled -eq $after.secureBootEnabled -and $bootBefore.codeIntegrityOptions -eq $after.codeIntegrityOptions)
    $result.cacheRemoved=(-not(Test-Path -LiteralPath $cache))
    $result.sourceRestored=(Test-Path -LiteralPath (Join-Path $selected 'alpha.bin')) -and -not(Test-Path -LiteralPath $backing)
    $result.workerSha256=(Get-FileHash -LiteralPath $worker).Hash
    $result.sourceSha256=(Get-FileHash -LiteralPath (Join-Path $root ('tools\'+$EvidenceGroup+'\worker.cpp'))).Hash
    $result.passed=[bool]($result.passed -and $result.bootUnchanged -and $result.cacheRemoved -and $result.sourceRestored)
    $result|ConvertTo-Json -Depth 9|Set-Content -LiteralPath (Join-Path $runRoot 'result.json') -Encoding utf8
}
$result|ConvertTo-Json -Depth 9
if(-not $result.passed){throw ('Folder cache proof failed; inspect '+$runRoot)}
