param([switch]$ScmRecovery)
$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
    throw 'Managed-session VM test requires Windows administrator rights; no storage was changed.'
}
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$runLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$runId=[Guid]::NewGuid().ToString('N')
$hostRun=Join-Path $lab ('managed-'+$runId)
$guestRun='C:\SuperHDDLab\managed-'+$runId
$guestBinaries='C:\Program Files\SuperHDDLab\Managed-'+$runId
$resultName=if($ScmRecovery){'scm-recovery'}else{'managed-session'}
$probeName=if($ScmRecovery){'scm_recovery_probe.exe'}else{'managed_session_probe.exe'}
$resultPath=Join-Path $lab ($resultName+'.json')
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$runId;phase='starting';passed=$false;hostDriverInstalled=$false;physicalMediaProof=$false;error=$null}
$session=$null;$vm=$null;$parents=@()
function Save-Result {$result|ConvertTo-Json -Depth 24|Set-Content -LiteralPath $resultPath -Encoding utf8NoBOM}
try{
    if(Test-Path -LiteralPath $resultPath){Move-Item -LiteralPath $resultPath -Destination (Join-Path $lab ($resultName+'-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff')+'.json'))}
    $configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $configuration.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Wrong disposable VM.'}
    $result.vmId=$vm.Id.ToString();Save-Result
    New-Item -ItemType Directory -Path $hostRun|Out-Null
    if($vm.State -eq 'Off'){Start-VM -VM $vm|Out-Null}
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    for($attempt=0;$attempt -lt 40;$attempt++){
        try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 3}
    }
    if(-not $session){throw 'Disposable guest not ready.'}
    $result.baseVolumes=Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()
    $result.preparation=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries,$vm.Id.ToString() -ScriptBlock {
        param($Run,$Binaries,$VmId)
        $ErrorActionPreference='Stop'
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $VmId){throw 'Guest identity failed.'}
        if($Run -notmatch '^C:\\SuperHDDLab\\managed-[a-f0-9]{32}$' -or $Binaries -notmatch '^C:\\Program Files\\SuperHDDLab\\Managed-[a-f0-9]{32}$'){throw 'Guest paths failed.'}
        if(Get-Volume -DriveLetter S,K,I -ErrorAction SilentlyContinue){throw 'Managed fixture drive letter already in use.'}
        if((& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json).cache.attached){throw 'Existing kernel session must be stopped first.'}
        New-Item -ItemType Directory -Path $Run,$Binaries|Out-Null
        [pscustomobject]@{driverHash=(Get-FileHash C:\SuperHDDLab\driver.sys).Hash;signature=(Get-AuthenticodeSignature C:\SuperHDDLab\driver.sys).Status.ToString()}
    }
    if($result.preparation.signature -ne 'Valid' -or $result.preparation.driverHash -ne (Get-FileHash (Join-Path $lab 'signed-driver.sys')).Hash){throw 'Wrong or unsigned guest driver.'}
    $result.phase='creating-run-owned-virtual-parents';Save-Result
    # Each initialized guest disk is identified as the single newly arrived disk
    # after adding this exact newly created Hyper-V file. No guessed disk number.
    foreach($entry in @(@{name='source-parent.vhdx';letter='S';label='SHDD-M-SOURCE-'},@{name='cache-parent.vhdx';letter='K';label='SHDD-M-CACHE-'})){
        # Serialize inside the guest. Remoted scalar strings carry PowerShell
        # metadata which must never become JSON disk-identity objects on host.
        $beforeJson=Invoke-Command -Session $session -ScriptBlock {ConvertTo-Json -InputObject ([string[]]@(Get-Disk|ForEach-Object {$_.UniqueId})) -Compress}
        $path=Join-Path $hostRun $entry.name
        New-VHD -Path $path -Dynamic -SizeBytes 1GB|Out-Null
        $parents+=@{path=$path;letter=$entry.letter;beforeJson=[string]$beforeJson}
        $used=@($vm|Get-VMHardDiskDrive|Where-Object ControllerType -eq 'SCSI'|Select-Object -ExpandProperty ControllerLocation)
        $location=0..63|Where-Object {$_ -notin $used}|Select-Object -First 1
        if($null -eq $location){throw 'No free isolated VM SCSI location.'}
        Add-VMHardDiskDrive -VM $vm -ControllerType SCSI -ControllerNumber 0 -ControllerLocation $location -Path $path
        $identity=Invoke-Command -Session $session -ArgumentList $beforeJson,$entry.letter,($entry.label+$runId.Substring(0,8)) -ScriptBlock {
            param($BeforeJson,$Letter,$Label)
            $ErrorActionPreference='Stop';$disk=$null
            # Windows PowerShell 5 emits a JSON array as one pipeline object.
            # An @() wrapper here would add a second array dimension.
            $Before=ConvertFrom-Json -InputObject $BeforeJson
            if(-not $Before.Count -or @($Before|Where-Object {$_ -isnot [string]}).Count){throw ('Invalid baseline disk identities: '+$BeforeJson)}
            for($attempt=0;$attempt -lt 30;$attempt++){
                Update-HostStorageCache
                $new=@(Get-Disk|Where-Object {$_.UniqueId -notin $Before})
                if($new.Count -eq 1){$disk=$new[0];break}
                if($new.Count -gt 1){throw ('Ambiguous new guest disk; refusing initialization. Baseline: '+$BeforeJson+'; current: '+(ConvertTo-Json -InputObject @($new|Select-Object Number,UniqueId,BusType,Size) -Compress))}
                Start-Sleep -Milliseconds 300
            }
            $shape=$disk|Select-Object Number,UniqueId,Size,BusType,PartitionStyle,IsBoot,IsSystem,IsOffline,IsReadOnly
            if(-not $disk -or $disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 1GB -or $disk.PartitionStyle -notin 'RAW','MBR' -or $disk.BusType -notin 'SCSI','SAS','Virtual' -or @($disk|Get-Partition -ErrorAction SilentlyContinue).Count){throw ('New guest parent is not the exact empty 1 GiB virtual data disk: '+($shape|ConvertTo-Json -Compress))}
            if(Get-Volume -DriveLetter $Letter -ErrorAction SilentlyContinue){throw 'Guest letter collision.'}
            $id=$disk.UniqueId
            if($disk.IsOffline){$disk|Set-Disk -IsOffline $false}
            if($disk.IsReadOnly){$disk|Set-Disk -IsReadOnly $false}
            $initialized=if($disk.PartitionStyle -eq 'RAW'){$disk|Initialize-Disk -PartitionStyle MBR -PassThru}else{$disk}
            $initialized|New-Partition -UseMaximumSize -DriveLetter $Letter|Format-Volume -FileSystem NTFS -NewFileSystemLabel $Label -Confirm:$false|Out-Null
            $partition=Get-Partition -DriveLetter $Letter;$check=$partition|Get-Disk;$volume=$partition|Get-Volume
            if($check.UniqueId -ne $id -or $volume.FileSystemLabel -ne $Label){throw 'Parent identity changed during creation.'}
            [pscustomobject]@{uniqueId=$id;number=$check.Number;volume=$volume.Path;letter=$Letter;label=$Label;size=$check.Size;controllerBus=$shape.BusType.ToString();initialPartitionStyle=$shape.PartitionStyle.ToString()}
        }
        $parents[-1].guest=$identity
    }
    $result.parents=$parents;Save-Result
    Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'Helpers',$vm.Id.ToString()|Out-Null
    $result.source=Invoke-Command -Session $session -ArgumentList $guestRun,$runId -ScriptBlock {
        param($Run,$RunId)
        $ErrorActionPreference='Stop'
        $path='S:\source-'+$RunId+'.vhdx'
        if(Test-Path -LiteralPath $path){throw 'Source fixture already exists.'}
        $commands=@"
create vdisk file="$path" maximum=256 type=fixed
select vdisk file="$path"
attach vdisk
exit
"@
        $commands|Set-Content -LiteralPath (Join-Path $Run 'create-source.txt') -Encoding ASCII
        & diskpart.exe /s (Join-Path $Run 'create-source.txt') *> (Join-Path $Run 'create-source.log')
        if($LASTEXITCODE){throw 'Source child creation failed.'}
        $disk=Get-DiskImage -ImagePath $path|Get-Disk
        if($disk.BusType -ne 'File Backed Virtual' -or $disk.Size -ne 256MB -or $disk.IsBoot -or $disk.IsSystem -or $disk.PartitionStyle -notin 'RAW','MBR' -or @($disk|Get-Partition -ErrorAction SilentlyContinue).Count){throw ('Wrong new empty source child disk: '+($disk|Select-Object Number,Size,BusType,PartitionStyle|ConvertTo-Json -Compress))}
        $id=$disk.UniqueId
        if($disk.IsOffline){$disk|Set-Disk -IsOffline $false}
        if($disk.IsReadOnly){$disk|Set-Disk -IsReadOnly $false}
        $initialized=if($disk.PartitionStyle -eq 'RAW'){$disk|Initialize-Disk -PartitionStyle MBR -PassThru}else{$disk}
        $initialized|New-Partition -UseMaximumSize -DriveLetter I|Format-Volume -FileSystem NTFS -NewFileSystemLabel ('SHDD-M-CHILD-'+$RunId.Substring(0,8)) -Confirm:$false|Out-Null
        $device=[LabVolume]::Device('I:')
        $node=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device})
        if($node.Count -ne 1 -or $node[0].InstanceId -notlike 'STORAGE\Volume\*'){throw 'Cannot identify exact new child volume devnode.'}
        [LabPnp]::Enroll($device)
        & pnputil.exe /restart-device $node[0].InstanceId *> (Join-Path $Run 'enroll-source.log')
        if($LASTEXITCODE){throw 'Source filter restart failed.'}
        $disk=@(Get-Disk|Where-Object UniqueId -eq $id)
        if($disk.Count -ne 1 -or $disk[0].IsBoot -or $disk[0].IsSystem){throw 'Source identity changed after enrollment.'}
        $partition=@($disk[0]|Get-Partition|Where-Object Type -ne 'Reserved')
        if($partition.Count -ne 1 -or ($partition[0]|Get-Volume).FileSystemLabel -ne ('SHDD-M-CHILD-'+$RunId.Substring(0,8))){throw 'Child filesystem identity changed.'}
        if(-not(Get-Volume -DriveLetter I -ErrorAction SilentlyContinue)){$partition[0]|Set-Partition -NewDriveLetter I}
        if((Get-Partition -DriveLetter I|Get-Disk).UniqueId -ne $id){throw 'Source drive letter changed identity.'}
        [pscustomobject]@{path=$path;uniqueId=$id;device=[LabVolume]::Device('I:');node=$node[0].InstanceId;number=$disk[0].Number}
    }
    $files=@{'target\release\SuperHDDService.exe'='SuperHDDService.exe'}
    $files['target\release\examples\'+$probeName]=$probeName
    $result.binaryHashes=@()
    foreach($file in $files.GetEnumerator()){
        $source=Join-Path $root $file.Key
        Copy-Item -ToSession $session -LiteralPath $source -Destination (Join-Path $guestBinaries $file.Value)
        $hash=(Get-FileHash -LiteralPath $source).Hash
        $guestHash=Invoke-Command -Session $session -ArgumentList (Join-Path $guestBinaries $file.Value) -ScriptBlock {param($File)(Get-FileHash -LiteralPath $File).Hash}
        if($guestHash -ne $hash){throw 'Guest managed-service/probe image differs from the current built image.'}
        $result.binaryHashes+=@{name=$file.Value;sha256=$hash;guestHashVerified=$true}
    }
    $request=@{sourceVolume='I:';containerRoot=('K:\managed-'+$runId);budgetBytes=256MB;acknowledgedDisposableSource=$true;isolatedLab=@{vmId=$vm.Id.ToString();runId=$runId;sourceParentVolume=$parents[0].guest.volume;cacheParentVolume=$parents[1].guest.volume}}
    $request|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $hostRun 'request.json') -Encoding utf8NoBOM
    Copy-Item -ToSession $session -LiteralPath (Join-Path $hostRun 'request.json') -Destination (Join-Path $guestRun 'request.json')
    $result.phase='actual-service-owned-vhd-driver-data-cycle';Save-Result
    $result.probe=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries,$probeName -ScriptBlock {
        param($Run,$Binaries,$Probe)
        $ErrorActionPreference='Stop'
        $process=Start-Process -FilePath (Join-Path $Binaries $Probe) -ArgumentList ('"'+(Join-Path $Binaries 'SuperHDDService.exe')+'" "'+(Join-Path $Run 'request.json')+'" "'+(Join-Path $Run 'probe.json')+'"') -RedirectStandardOutput (Join-Path $Run 'probe.log') -RedirectStandardError (Join-Path $Run 'probe-error.log') -WindowStyle Hidden -PassThru
        try{
            $ownedProcessHandle=$process.Handle
            if(-not $process.WaitForExit(240000)){$process.Kill();throw 'Managed-session probe exceeded its bounded deadline.'}
            $process.Refresh()
            if($process.ExitCode -ne 0){throw 'Managed-session probe failed; inspect its guest logs.'}
        }finally{$process.Dispose()}
        Get-Content -LiteralPath (Join-Path $Run 'probe.json') -Raw|ConvertFrom-Json
    }
    if(-not $result.probe.passed -or -not $result.probe.ownedFixturesRemoved){throw 'Native managed-session/data cleanup checks failed.'}
    $result.passed=$true
}catch{$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}finally{
    $result.parents=$parents
    if($session){
        try{
            $result.guestCleanup=Invoke-Command -Session $session -ArgumentList $guestRun,$runId,([bool]$ScmRecovery) -ScriptBlock {
                param($Run,$RunId,$ScmRecovery)
                $ErrorActionPreference='Stop'
                $state=& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json
                if($state.cache.attached){throw 'Kernel still attached: preserve run-owned disks for diagnosis.'}
                $owned=Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like ('C:\Program Files\SuperHDDLab\Managed-'+$RunId+'\*')}
                if($owned){throw 'Owned process remains: preserve disks for diagnosis.'}
                if($ScmRecovery -and ((Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'") -or (Test-Path -LiteralPath 'C:\ProgramData\SuperHDDManagedLab'))){throw 'Owned SCM service/state remains: preserve disks for diagnosis.'}
                $path='S:\source-'+$RunId+'.vhdx'
                if(Test-Path -LiteralPath $path){
                    $image=Get-DiskImage -ImagePath $path
                    if($image.Attached){
                        $disk=$image|Get-Disk;$partition=@($disk|Get-Partition|Where-Object Type -ne 'Reserved')
                        if($disk.IsBoot -or $disk.IsSystem -or $disk.BusType -ne 'File Backed Virtual' -or $partition.Count -ne 1 -or ($partition[0]|Get-Volume).FileSystemLabel -ne ('SHDD-M-CHILD-'+$RunId.Substring(0,8))){throw 'Cleanup child identity mismatch.'}
                        $device=[LabVolume]::Device('I:')
                        if((Get-Partition -DriveLetter I|Get-Disk).UniqueId -ne $disk.UniqueId){throw 'Cleanup letter belongs to another source.'}
                        $node=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device})
                        if($node.Count -ne 1){throw 'Cleanup source node ambiguous.'}
                        [LabPnp]::Remove($device)
                        & pnputil.exe /restart-device $node[0].InstanceId *> (Join-Path $Run 'remove-filter.log')
                        if($LASTEXITCODE){throw 'Source filter removal restart failed.'}
                        Dismount-DiskImage -ImagePath $path|Out-Null
                    }
                    if((Get-DiskImage -ImagePath $path).Attached){throw 'Owned child remains mounted.'}
                    Remove-Item -LiteralPath $path
                }
                [pscustomobject]@{cacheDetached=$true;childRemoved=(-not(Test-Path -LiteralPath $path));ownedProcessesAbsent=$true;ownedScmServiceAbsent=($ScmRecovery -and -not(Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"));protectedStateRemoved=($ScmRecovery -and -not(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDManagedLab'))}
            }
        }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
        try{Copy-Item -FromSession $session -LiteralPath $guestRun -Destination (Join-Path $hostRun 'guest-logs') -Recurse}catch{$result.copyError=$_.Exception.Message;$result.passed=$false}
        Remove-PSSession $session -ErrorAction SilentlyContinue
    }
    # Detach only exact new host files after the guest confirmed shutdown of its
    # cache and source. Failure leaves bounded fixtures for diagnosis.
    if($result.guestCleanup.cacheDetached -and $result.guestCleanup.childRemoved){
        try{
            foreach($parent in $parents){
                $absolute=[IO.Path]::GetFullPath($parent.path)
                if([IO.Path]::GetDirectoryName($absolute) -ne [IO.Path]::GetFullPath($hostRun) -or [IO.Path]::GetFileName($absolute) -notin 'source-parent.vhdx','cache-parent.vhdx'){throw 'Cleanup host path escaped the new run directory.'}
                $attachment=@($vm|Get-VMHardDiskDrive|Where-Object Path -eq $absolute)
                if($attachment.Count -gt 1){throw 'Ambiguous VM attachment.'}
                if($attachment.Count -eq 1){$attachment[0]|Remove-VMHardDiskDrive}
                if(@(Get-VM|Get-VMHardDiskDrive|Where-Object Path -eq $absolute).Count){throw 'Owned parent remains attached to a VM.'}
                Remove-Item -LiteralPath $absolute
            }
            $result.hostParentsRemoved=$true
        }catch{$result.hostCleanupError=$_.Exception.Message;$result.passed=$false}
    }
    $result.passed=[bool]($result.passed -and $result.hostParentsRemoved)
    Save-Result;$runLock.Dispose()
}
try{
    & (Join-Path $root 'scripts\stop-kernel-lab.ps1')
    $result.finalHost=Get-Content -LiteralPath (Join-Path $lab 'final-host-state.json') -Raw|ConvertFrom-Json
    $result.passed=[bool]($result.passed -and $result.finalHost.passed -and $result.finalHost.vmState -eq 'Off')
}catch{$result.shutdownError=$_.Exception.Message;$result.passed=$false}
$result.phase=if($result.passed){'completed'}else{'failed'}
$result.sourceHashes=@('crates\storage-control\Cargo.toml','crates\storage-control\src\lib.rs','crates\storage-control\src\lab.rs','crates\storage-control\src\lab_windows.rs','crates\storage-control\src\windows.rs','crates\storage-control\src\raw_volume.rs','crates\storage-control\src\journal.rs','crates\storage-control\src\format.rs','crates\storage-control\src\session.rs','crates\platform\src\managed_cache.rs','crates\platform\src\raw_lab.rs','services\superhdd-service\src\main.rs','services\superhdd-service\src\lab_control.rs','crates\platform\examples\managed_session_probe.rs','scripts\test-managed-session.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('crates\storage-control\src\protected.rs','crates\storage-control\src\recovery.rs','services\superhdd-service\src\managed_host.rs','crates\platform\examples\scm_recovery_probe.rs'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('services\superhdd-service\Cargo.toml','crates\platform\Cargo.toml'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
Save-Result
if(-not $result.passed){throw (@($result.error,$result.cleanupError,$result.shutdownError,'Managed-session verification did not pass.')|Where-Object {$_}|Select-Object -First 1)}
