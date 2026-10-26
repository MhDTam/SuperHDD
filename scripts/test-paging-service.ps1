param([switch]$HostHddFiles)
$SourcePageFile=$true
$ScmRecovery=$true
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
$resultName='paging-service'
$probeName='paging_service_probe.exe'
$resultPath=Join-Path $lab ($resultName+'.json')
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$runId;phase='starting';passed=$false;hostDriverInstalled=$false;physicalMediaProof=$false;error=$null}
$session=$null;$vm=$null;$parents=@();$sourcePageConfigured=$false
$hostSource=if($HostHddFiles){'D:\SuperHDD-DirectTest-'+$runId+'.vhdx'}else{Join-Path $hostRun 'source-parent.vhdx'}
function Save-Result {$result|ConvertTo-Json -Depth 24|Set-Content -LiteralPath $resultPath -Encoding utf8NoBOM}
try{
    if(Test-Path -LiteralPath $resultPath){Move-Item -LiteralPath $resultPath -Destination (Join-Path $lab ($resultName+'-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff')+'.json'))}
    $configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $configuration.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Wrong disposable VM.'}
    $result.vmId=$vm.Id.ToString();Save-Result
    New-Item -ItemType Directory -Path $hostRun|Out-Null
    if($HostHddFiles){
        $hdd=Get-Partition -DriveLetter D|Get-Disk
        $ssd=Get-Partition -DriveLetter M|Get-Disk
        if($hdd.IsBoot -or $hdd.IsSystem -or $hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $ssd.FriendlyName -notmatch '980 PRO' -or $hdd.Number -eq $ssd.Number -or (Get-Volume -DriveLetter D).SizeRemaining -lt 5GB){throw 'Current-machine HDD/SSD file-test identity mismatch.'}
        if(Test-Path -LiteralPath $hostSource){throw 'New HDD fixture path already exists.'}
        $result.hostFileBacking=@{scope='V8 kernel/service inside isolated guest; new VHDX files on real host HDD/SSD; not transparent host acceleration';sourcePath=$hostSource;sourceDisk=[string]$hdd.UniqueId;sourceVolume=[string](Get-Volume -DriveLetter D).UniqueId;sourceModel=$hdd.FriendlyName;cacheDisk=[string]$ssd.UniqueId;cacheVolume=[string](Get-Volume -DriveLetter M).UniqueId;cacheModel=$ssd.FriendlyName;sourceVirtualBytes=1GB;cacheParentVirtualBytes=1GB}
        $preflight=Join-Path $root 'target\paging-kernel\release\examples\local_preflight.exe'
        if(-not(Test-Path -LiteralPath $preflight)){throw 'Build the read-only local_preflight example first.'}
        $hostFacts=& $preflight|ConvertFrom-Json
        if($LASTEXITCODE -or -not $hostFacts.readOnly){throw 'Read-only host preflight failed.'}
        $hostFacts|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $hostRun 'host-preflight.json') -Encoding utf8NoBOM
        $result.hostFileBacking.preflight='host-preflight.json'
        $result.hostFileBacking.preflightToolSha256=(Get-FileHash -LiteralPath $preflight).Hash
        Save-Result
    }
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
        $path=if($entry.letter -eq 'S'){$hostSource}else{Join-Path $hostRun $entry.name}
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
    $files=@{'target\paging-kernel\release\SuperHDDService.exe'='SuperHDDService.exe'}
    $files['target\paging-kernel\release\examples\'+$probeName]=$probeName
    $files['target\paging-kernel\release\examples\paging_session_probe.exe']='paging_session_probe.exe'
    $result.binaryHashes=@()
    foreach($file in $files.GetEnumerator()){
        $source=Join-Path $root $file.Key
        Copy-Item -ToSession $session -LiteralPath $source -Destination (Join-Path $guestBinaries $file.Value)
        $hash=(Get-FileHash -LiteralPath $source).Hash
        $guestHash=Invoke-Command -Session $session -ArgumentList (Join-Path $guestBinaries $file.Value) -ScriptBlock {param($File)(Get-FileHash -LiteralPath $File).Hash}
        if($guestHash -ne $hash){throw 'Guest managed-service/probe image differs from the current built image.'}
        $result.binaryHashes+=@{name=$file.Value;sha256=$hash;guestHashVerified=$true}
    }
    $helperText=Get-Content -LiteralPath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -Raw
    $match=[regex]::Match($helperText,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@")
    if(-not $match.Success){throw 'Cannot find the native PnP helper.'}
    $code=$match.Groups[1].Value.Replace('LabVolume','DirectVolume').Replace('LabPnp','DirectPnp').Replace('SuperHDDRawLab','SuperHDDCacheV8')
    $driverPath=Join-Path $root 'artifacts\driver-paging-vm\SuperHDDCacheDriver.sys'
    Copy-Item -ToSession $session -LiteralPath $driverPath -Destination (Join-Path $guestBinaries 'driver-v8.sys')
    $unsignedHash=(Get-FileHash -LiteralPath $driverPath).Hash
    $result.directDriver=Invoke-Command -Session $session -ArgumentList $guestBinaries,$guestRun,$vm.Id.ToString(),$runId,$code,$unsignedHash -ScriptBlock {
        param($Binaries,$Run,$VmId,$RunId,$Code,$UnsignedHash)
        $ErrorActionPreference='Stop'
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $VmId){throw 'Wrong VM.'}
        if((Get-FileHash -LiteralPath (Join-Path $Binaries 'driver-v8.sys')).Hash -ne $UnsignedHash){throw 'Driver copy hash mismatch.'}
        if(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheV8'"){throw 'A v8 driver already exists.'}
        $disk=Get-Partition -DriveLetter S|Get-Disk
        $volume=Get-Volume -DriveLetter S
        if($disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 1GB -or $disk.BusType -ne 'SAS' -or $volume.FileSystemLabel -ne ('SHDD-M-SOURCE-'+$RunId.Substring(0,8))){throw 'Wrong new SAS source.'}
        $cert=Get-ChildItem Cert:\LocalMachine\My|Where-Object {$_.Subject -eq ('CN=SuperHDD Disposable VM '+$VmId) -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date)}|Select-Object -First 1
        if(-not $cert){throw 'Existing lab signing certificate unavailable.'}
        & C:\SuperHDDLab\signtool.exe sign /ph /fd SHA256 /sm /s My /sha1 $cert.Thumbprint (Join-Path $Binaries 'driver-v8.sys') *> (Join-Path $Run 'sign-v8.log')
        if($LASTEXITCODE -or (Get-AuthenticodeSignature (Join-Path $Binaries 'driver-v8.sys')).Status -ne 'Valid'){throw 'Guest-only v8 signing failed.'}
        Add-Type -TypeDefinition $Code
        $device=[DirectVolume]::Device('S:')
        $node=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device})
        if($node.Count -ne 1 -or $node[0].InstanceId -notlike 'STORAGE\Volume\*'){throw 'Source devnode ambiguous.'}
        & sc.exe create SuperHDDCacheV8 type= kernel start= demand binPath= (Join-Path $Binaries 'driver-v8.sys') *> (Join-Path $Run 'create-driver.log')
        if($LASTEXITCODE){throw 'V8 service creation failed.'}
        [DirectPnp]::Enroll($device)
        & pnputil.exe /restart-device $node[0].InstanceId *> (Join-Path $Run 'start-v8.log')
        if($LASTEXITCODE){throw 'V8 source restart failed.'}
        [pscustomobject]@{unsignedSha256=$UnsignedHash;signedSha256=(Get-FileHash (Join-Path $Binaries 'driver-v8.sys')).Hash;signature='Valid';sourceDisk=$disk.Number;sourceBus=$disk.BusType.ToString();sourceNode=$node[0].InstanceId;sourceDevice=$device;sourceVolume=$volume.Path}
    }
    $result.source=@{node=$result.directDriver.sourceNode;number=$result.directDriver.sourceDisk;device=$result.directDriver.sourceDevice;volume=$result.directDriver.sourceVolume}
    Copy-Item -FromSession $session -LiteralPath (Join-Path $guestBinaries 'driver-v8.sys') -Destination (Join-Path $hostRun 'signed-driver-v8.sys')
    if($SourcePageFile){
        $result.phase='create-real-pagefile-on-guest-source';Save-Result
        Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {
            param($Run)
            $ErrorActionPreference='Stop';$key='HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
            $computer=Get-CimInstance Win32_ComputerSystem
            @{automatic=[bool]$computer.AutomaticManagedPagefile;paging=[string[]](Get-ItemPropertyValue $key PagingFiles)}|ConvertTo-Json|Set-Content (Join-Path $Run 'source-page-policy.json')
            $computer|Set-CimInstance -Property @{AutomaticManagedPagefile=$false}
            Set-ItemProperty $key PagingFiles -Value ([string[]]@('C:\pagefile.sys 0 0','S:\pagefile.sys 128 128'))
            & shutdown.exe /r /t 3|Out-Null
        }
        $sourcePageConfigured=$true;Remove-PSSession $session;$session=$null;Start-Sleep -Seconds 10
        for($i=0;$i -lt 60;$i++){try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 2}}
        if(-not $session){throw 'Guest did not return for source page-file proof'}
        $result.sourcePageFile=Invoke-Command -Session $session -ScriptBlock {
            $page=@(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys')
            if($page.Count -ne 1 -or $page[0].AllocatedBaseSize -ne 128){throw 'Actual source page file absent'}
            $page|Select-Object Name,AllocatedBaseSize
        }
        Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()|Out-Null
        Invoke-Command -Session $session -ArgumentList $code -ScriptBlock {param($Code)Add-Type -TypeDefinition $Code}
        Save-Result
    }
    $request=@{sourceVolume='S:';containerRoot=('K:\managed-'+$runId);budgetBytes=256MB;acknowledgedDisposableSource=$true;isolatedLab=@{vmId=$vm.Id.ToString();runId=$runId;sourceParentVolume=$parents[0].guest.volume;cacheParentVolume=$parents[1].guest.volume}}
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
            if($process.ExitCode -ne 0 -and -not(Test-Path (Join-Path $Run 'probe.json'))){throw 'Managed-session probe failed without a report; inspect its guest logs.'}
        }finally{$process.Dispose()}
        Get-Content -LiteralPath (Join-Path $Run 'probe.json') -Raw|ConvertFrom-Json
    }
    if(-not $result.probe.passed -or -not $result.probe.ownedFixturesRemoved){throw 'Native managed-session/data cleanup checks failed.'}
    $kernelRequest=@{sourceVolume=$request.sourceVolume;containerRoot=$request.containerRoot;budgetBytes=128MB;isolatedLab=$request.isolatedLab}
    $kernelRequest|ConvertTo-Json -Depth 5|Set-Content (Join-Path $hostRun 'application-request.json') -Encoding utf8NoBOM
    Copy-Item -ToSession $session -LiteralPath (Join-Path $hostRun 'application-request.json') -Destination (Join-Path $guestRun 'application-request.json')
    $result.phase='buffered-mapped-workload-after-scm-cleanup';Save-Result
    $result.applicationProbeStarted=$true;Save-Result
    $result.applicationKernelProbe=Invoke-Command -Session $session -ArgumentList $guestRun,$guestBinaries -ScriptBlock {
        param($Run,$Binaries)
        $ErrorActionPreference='Stop'
        $process=Start-Process -FilePath (Join-Path $Binaries 'paging_session_probe.exe') -ArgumentList ('"'+(Join-Path $Run 'application-request.json')+'" "'+(Join-Path $Run 'application-proof.json')+'"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $Run 'application.log') -RedirectStandardError (Join-Path $Run 'application-error.log')
        if(-not $process.WaitForExit(300000)){$process.Kill();throw 'Owned application workload timed out'}
        if(-not(Test-Path (Join-Path $Run 'application-proof.json'))){throw 'Application workload produced no report'}
        Get-Content (Join-Path $Run 'application-proof.json') -Raw|ConvertFrom-Json
    }
    if(-not $result.applicationKernelProbe.passed -or -not $result.applicationKernelProbe.ownedFixturesRemoved){throw 'Paging application workload did not pass'}
    $result.passed=$true
}catch{$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}finally{
    $result.parents=$parents
    if($session){
        if($sourcePageConfigured -and $result.probe.ownedFixturesRemoved -and (-not $result.applicationProbeStarted -or $result.applicationKernelProbe.ownedFixturesRemoved)){
            try{
                Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {
                    param($Run)
                    $ErrorActionPreference='Stop';$backup=Get-Content (Join-Path $Run 'source-page-policy.json') -Raw|ConvertFrom-Json
                    Get-CimInstance Win32_ComputerSystem|Set-CimInstance -Property @{AutomaticManagedPagefile=[bool]$backup.automatic}
                    Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' PagingFiles -Value ([string[]]$backup.paging)
                    & shutdown.exe /r /t 3|Out-Null
                }
                Remove-PSSession $session;$session=$null;Start-Sleep -Seconds 10
                for($i=0;$i -lt 60;$i++){try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 2}}
                if(-not $session){throw 'Guest did not return after page-file restoration'}
                Invoke-Command -Session $session -ScriptBlock {if(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys'){throw 'Source page file remains active'}}
                Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()|Out-Null
                Invoke-Command -Session $session -ArgumentList $code -ScriptBlock {param($Code)Add-Type -TypeDefinition $Code}
                $result.sourcePageFileRestored=$true
            }catch{$result.pageFileRestoreError=$_.Exception.Message;$result.passed=$false}
        }
        try{
            $result.guestCleanup=Invoke-Command -Session $session -ArgumentList $guestRun,$runId,([bool]$ScmRecovery) -ScriptBlock {
                param($Run,$RunId,$ScmRecovery)
                $ErrorActionPreference='Stop'
                $pagingTool=Join-Path ('C:\Program Files\SuperHDDLab\Managed-'+$RunId) 'paging_session_probe.exe'
                $pagingState=& $pagingTool status|ConvertFrom-Json
                if($LASTEXITCODE -or $pagingState.cache.attached){throw 'Paging cache still attached; preserve owned disks'}
                $state=& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json
                if($state.cache.attached){throw 'Kernel still attached: preserve run-owned disks for diagnosis.'}
                $owned=Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like ('C:\Program Files\SuperHDDLab\Managed-'+$RunId+'\*')}
                if($owned){throw 'Owned process remains: preserve disks for diagnosis.'}
                if($ScmRecovery -and ((Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'") -or (Test-Path -LiteralPath 'C:\ProgramData\SuperHDDPaging'))){throw 'Owned SCM service/state remains: preserve disks for diagnosis.'}
                $device=[DirectVolume]::Device('S:')
                $volume=Get-Volume -DriveLetter S
                $disk=Get-Partition -DriveLetter S|Get-Disk
                if($disk.IsBoot -or $disk.IsSystem -or $disk.Size -ne 1GB -or $volume.FileSystemLabel -ne ('SHDD-M-SOURCE-'+$RunId.Substring(0,8))){throw 'Cleanup source mismatch.'}
                $node=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device})
                if($node.Count -ne 1){throw 'Cleanup node ambiguous.'}
                [DirectPnp]::Remove($device)
                & pnputil.exe /restart-device $node[0].InstanceId *> (Join-Path $Run 'remove-v8-filter.log')
                if($LASTEXITCODE){throw 'V8 filter removal failed.'}
                # A PnP filter is removed by PnP, not by SCM STOP (1052).
                # Remove its registration after removing the exact source filter;
                # the final VM shutdown releases any remaining driver image.
                & sc.exe delete SuperHDDCacheV8 *> (Join-Path $Run 'delete-v8-driver.log')
                if($LASTEXITCODE){throw 'V8 driver service deletion failed.'}
                # SCM can retain a marked-for-deletion PnP registration until the
                # loaded image is released. Prove its absence after cold boot.
                [pscustomobject]@{cacheDetached=$true;childRemoved=$true;ownedProcessesAbsent=$true;ownedScmServiceAbsent=(-not(Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"));protectedStateRemoved=(-not(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDPaging'));directDriverDeletionRequested=$true;sourceFilterRemoved=$true}
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
                $expectedParent=if($parent.letter -eq 'S'){$hostSource}else{Join-Path $hostRun 'cache-parent.vhdx'}
                if($absolute -ne [IO.Path]::GetFullPath($expectedParent)){throw 'Cleanup host path escaped the exact new run file.'}
                if($HostHddFiles -and $parent.letter -eq 'S' -and [string](Get-Partition -DriveLetter D|Get-Disk).UniqueId -ne $result.hostFileBacking.sourceDisk){throw 'HDD identity changed; preserve fixture.'}
                if($HostHddFiles -and [string](Get-Partition -DriveLetter M|Get-Disk).UniqueId -ne $result.hostFileBacking.cacheDisk){throw 'SSD identity changed; preserve fixture.'}
                if($HostHddFiles -and ([string](Get-Volume -DriveLetter D).UniqueId -ne $result.hostFileBacking.sourceVolume -or [string](Get-Volume -DriveLetter M).UniqueId -ne $result.hostFileBacking.cacheVolume)){throw 'Host volume mapping changed; preserve fixture.'}
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
if($result.passed){
    $rebootLock=$null;$rebootSession=$null
    try{
        $rebootLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
        $vm=Get-VM -Id $configuration.vmId
        if($vm.State -ne 'Off' -or $vm.MemoryAssigned -ne 0){throw 'Cold-boot cleanup proof requires a fully stopped guest.'}
        Start-VM -VM $vm|Out-Null
        for($attempt=0;$attempt -lt 40;$attempt++){try{$rebootSession=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 3}}
        if(-not $rebootSession){throw 'Guest did not return for cold-boot cleanup verification.'}
        $result.coldBootCleanup=Invoke-Command -Session $rebootSession -ArgumentList $vm.Id.ToString() -ScriptBlock {
            param($VmId)
            $ErrorActionPreference='Stop'
            if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $VmId){throw 'Wrong guest after cold boot.'}
            if((Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheV8'") -or (Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'") -or (Test-Path -LiteralPath 'C:\ProgramData\SuperHDDPaging')){throw 'Owned driver/service/state survived cleanup.'}
            $refs=@(Get-PnpDevice -Class Volume|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data -contains 'SuperHDDCacheV8'})
            if($refs.Count){throw 'A volume still references the deleted v8 driver.'}
            [pscustomobject]@{passed=$true;driverRegistrationAbsent=$true;driverFiltersAbsent=$true;ownerServiceAbsent=$true;privateStateAbsent=$true}
        }
        Invoke-Command -Session $rebootSession -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()|Out-Null
    }catch{$result.coldBootCleanupError=$_.Exception.Message;$result.passed=$false}finally{
        if($rebootSession){Remove-PSSession $rebootSession}
        if($rebootLock){$rebootLock.Dispose()}
    }
    try{
        & (Join-Path $root 'scripts\stop-kernel-lab.ps1')
        $result.finalHost=Get-Content -LiteralPath (Join-Path $lab 'final-host-state.json') -Raw|ConvertFrom-Json
        $result.passed=[bool]($result.passed -and $result.coldBootCleanup.passed -and $result.finalHost.passed -and $result.finalHost.vmState -eq 'Off')
    }catch{$result.shutdownError=$_.Exception.Message;$result.passed=$false}
}
$result.phase=if($result.passed){'completed'}else{'failed'}
$result.sourceHashes=@('crates\storage-control\Cargo.toml','crates\storage-control\src\lib.rs','crates\storage-control\src\lab.rs','crates\storage-control\src\lab_windows.rs','crates\storage-control\src\windows.rs','crates\storage-control\src\raw_volume.rs','crates\storage-control\src\journal.rs','crates\storage-control\src\format.rs','crates\storage-control\src\session.rs','crates\platform\src\managed_cache.rs','crates\platform\src\raw_lab.rs','services\superhdd-service\src\main.rs','services\superhdd-service\src\lab_control.rs','crates\platform\examples\managed_session_probe.rs','scripts\test-paging-service.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('crates\storage-control\src\protected.rs','crates\storage-control\src\recovery.rs','services\superhdd-service\src\managed_host.rs','crates\platform\examples\scm_recovery_probe.rs'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('services\superhdd-service\Cargo.toml','crates\platform\Cargo.toml'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('Cargo.lock','crates\platform\src\lib.rs','crates\platform\src\ipc.rs','crates\platform\src\driver.rs','crates\platform\src\cache_runtime.rs','services\superhdd-service\src\paging_ops.rs','installer\enroll-paging-source.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('crates\platform\examples\local_preflight.rs'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@('crates\platform\src\paging_driver.rs','crates\platform\src\paging_cache.rs','services\superhdd-service\src\paging_host.rs','crates\platform\examples\paging_service_probe.rs','drivers\superhdd-cache-driver\v8\volume_driver.c','drivers\superhdd-cache-driver\v8\direct_source.inc','drivers\superhdd-cache-driver\v8\source_policy.h','drivers\superhdd-cache-driver\v8\protocol_v8.h','drivers\superhdd-cache-driver\v8\raw_backend.inc'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
$result.sourceHashes+=@(Get-ChildItem -LiteralPath (Join-Path $root 'drivers\superhdd-cache-driver') -File|Where-Object Extension -in '.h','.inc'|ForEach-Object{Get-FileHash -LiteralPath $_.FullName|Select-Object Path,Hash})
Save-Result
if(-not $result.passed){throw (@($result.error,$result.cleanupError,$result.shutdownError,'Managed-session verification did not pass.')|Where-Object {$_}|Select-Object -First 1)}
& (Join-Path $root 'scripts\restore-native-initiator-after-tests.ps1')

