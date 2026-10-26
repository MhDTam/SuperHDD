# Paging-capable v8 proof with a real page file on the cached source.
# All registration, signing, page-file changes and reboots occur in the owned VM.
param([switch]$HostHddFiles)
$ErrorActionPreference='Stop'
if(-not([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'VM access requires administrator consent.'}
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$lock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$run=[Guid]::NewGuid().ToString('N');$hostRun=Join-Path $lab ('paging-'+$run)
$guestRun='C:\SuperHDDLab\paging-'+$run
$record=Join-Path $root 'artifacts\paging-session.json'
$r=[ordered]@{runId=$run;phase='preflight';passed=$false;hostDriverInstalled=$false;hostPageFileChanged=$false;physicalDAccelerationActive=$false;error=$null}
$session=$null;$parents=@();$vm=$null;$pageChanged=$false
function Save {$r|ConvertTo-Json -Depth 15|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
function Step([string]$value){$r.phase=$value;Save}
function Connect-Owned {
    for($i=0;$i -lt 60;$i++) {try{$s=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;$valid=Invoke-Command -Session $s -ArgumentList $vm.Id.ToString() -ScriptBlock {param($id)$env:COMPUTERNAME -eq 'SHDD-LAB' -and (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -eq $id};if(-not $valid){Remove-PSSession $s;throw 'Wrong guest'};return $s}catch{Start-Sleep -Seconds 2}}
    throw 'Owned VM did not return.'
}
try{
    if(Test-Path -LiteralPath $record){Copy-Item -LiteralPath $record -Destination (Join-Path $lab ('paging-prior-'+[DateTime]::UtcNow.ToString('yyyyMMddHHmmssfff')+'.json'))}
    $cfg=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $cfg.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Only the provisioned network-isolated VM is accepted.'}
    $r.vmId=$vm.Id.ToString();$r.bootBefore=& (Join-Path $PSScriptRoot 'local-boot-state.ps1');Save
    if($HostHddFiles){
        $hdd=Get-Partition -DriveLetter D|Get-Disk;$ssd=Get-Partition -DriveLetter M|Get-Disk
        if($hdd.UniqueId -ne '5000CC2A22D4D7E0' -or $hdd.IsBoot -or $hdd.IsSystem -or $ssd.UniqueId -ne 'eui.002538B631A16109' -or $hdd.Number -eq $ssd.Number -or (Get-Volume -DriveLetter D).SizeRemaining -lt 3GB){throw 'Exact HDD/SSD fixture parents required'}
        $r.hostFileBacking=@{sourceDrive='D:';cacheDrive='M:';scope='VM-only filter; new file on host HDD, new cache files on host SSD; not transparent host D acceleration'};Save
    }
    $driver=Join-Path $root 'artifacts\driver-paging-vm\SuperHDDCacheDriver.sys'
    $probe=Join-Path $root 'target\paging-kernel\release\examples\paging_session_probe.exe'
    if(-not(Test-Path $driver) -or -not(Test-Path $probe)){throw 'Build the exact v8 VM driver and proof client first.'}
    $r.unsignedDriverSha256=(Get-FileHash $driver).Hash;$r.probeSha256=(Get-FileHash $probe).Hash
    New-Item -ItemType Directory -Path $hostRun|Out-Null
    $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
    if($vm.State -eq 'Off'){Start-VM -VM $vm|Out-Null}
    $session=Connect-Owned
    Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {param($dir)if(Get-Volume -DriveLetter S,K -ErrorAction SilentlyContinue){throw 'Fixture letters are in use'};New-Item -ItemType Directory -Path $dir|Out-Null}
    Step 'create-two-owned-guest-data-disks'
    foreach($item in @(@{letter='S';label='SHDD-M-SOURCE-'},@{letter='K';label='SHDD-M-CACHE-'})){
        $before=Invoke-Command -Session $session -ScriptBlock {ConvertTo-Json -InputObject ([string[]]@(Get-Disk|ForEach-Object {$_.UniqueId})) -Compress}
        $path=if($HostHddFiles -and $item.letter -eq 'S'){'D:\SuperHDD-PagingTest-'+$run+'.vhdx'}else{Join-Path $hostRun ($item.letter+'-parent.vhdx')}
        if(Test-Path -LiteralPath $path){throw 'New fixture already exists; preserved'}
        New-VHD -Path $path -Dynamic -SizeBytes 1GB|Out-Null
        $entry=@{path=$path;letter=$item.letter;creation=(Get-Item $path).CreationTimeUtc.ToFileTimeUtc()};$parents+=,$entry;$r.parents=$parents;Save
        $used=@($vm|Get-VMHardDiskDrive|Where-Object ControllerType -eq 'SCSI'|Select-Object -ExpandProperty ControllerLocation)
        $slot=0..63|Where-Object {$_ -notin $used}|Select-Object -First 1
        if($null -eq $slot){throw 'No free VM SCSI slot'}
        Add-VMHardDiskDrive -VM $vm -ControllerType SCSI -ControllerNumber 0 -ControllerLocation $slot -Path $path
        $entry.guest=Invoke-Command -Session $session -ArgumentList $before,$item.letter,($item.label+$run.Substring(0,8)) -ScriptBlock {
            param($before,$letter,$label)
            $ErrorActionPreference='Stop';$known=ConvertFrom-Json $before;$new=@()
            for($i=0;$i -lt 60;$i++){$new=@(Get-Disk|Where-Object {$_.UniqueId -notin $known});if($new.Count){break};Start-Sleep -Milliseconds 250}
            if($new.Count -ne 1 -or $new[0].Size -ne 1GB -or $new[0].BusType -ne 'SAS' -or $new[0].IsBoot -or $new[0].IsSystem -or $new[0].PartitionStyle -ne 'RAW'){throw 'One exact newly attached RAW SAS disk is required'}
            $new[0]|Set-Disk -IsOffline $false;$new[0]|Set-Disk -IsReadOnly $false
            $new[0]|Initialize-Disk -PartitionStyle GPT|Out-Null
            $part=New-Partition -DiskNumber $new[0].Number -UseMaximumSize -DriveLetter $letter
            $part|Format-Volume -FileSystem NTFS -NewFileSystemLabel $label -Confirm:$false|Out-Null
            [pscustomobject]@{volume=[string](Get-Volume -DriveLetter $letter).Path;uniqueId=[string]$new[0].UniqueId;number=$new[0].Number}
        };Save
    }
    Copy-Item -ToSession $session -LiteralPath $driver -Destination (Join-Path $guestRun 'driver-v8.sys')
    Copy-Item -ToSession $session -LiteralPath $probe -Destination (Join-Path $guestRun 'probe.exe')
    $helper=(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'host-test-pnp.cs') -Raw).Replace('SuperHDDHostTest','SuperHDDCacheV8')
    $request=@{sourceVolume='S:';containerRoot=('K:\managed-'+$run);budgetBytes=128MB;isolatedLab=@{vmId=$vm.Id.ToString();runId=$run;sourceParentVolume=[string]$parents[0].guest.volume;cacheParentVolume=[string]$parents[1].guest.volume}}
    $request|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $hostRun 'request.json') -Encoding utf8NoBOM
    Copy-Item -ToSession $session -LiteralPath (Join-Path $hostRun 'request.json') -Destination (Join-Path $guestRun 'request.json')
    Step 'install-guest-only-filter-and-pagefile'
    $r.install=Invoke-Command -Session $session -ArgumentList $guestRun,$vm.Id.ToString(),$run,$helper,$r.unsignedDriverSha256,$r.probeSha256 -ScriptBlock {
        param($dir,$vmid,$run,$code,$driverHash,$probeHash)
        $ErrorActionPreference='Stop'
        if((Get-FileHash (Join-Path $dir 'driver-v8.sys')).Hash -ne $driverHash -or (Get-FileHash (Join-Path $dir 'probe.exe')).Hash -ne $probeHash){throw 'Guest binaries changed'}
        if(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheV8'"){throw 'Existing v8 registration preserved'}
        $disk=Get-Partition -DriveLetter S|Get-Disk;$volume=Get-Volume -DriveLetter S
        if($disk.Size -ne 1GB -or $disk.BusType -ne 'SAS' -or $disk.IsBoot -or $disk.IsSystem -or $volume.FileSystemLabel -ne ('SHDD-M-SOURCE-'+$run.Substring(0,8))){throw 'Wrong disposable source'}
        $cert=Get-ChildItem Cert:\LocalMachine\My|Where-Object {$_.Subject -eq ('CN=SuperHDD Disposable VM '+$vmid) -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date)}|Select-Object -First 1
        if(-not $cert){throw 'Existing guest signing certificate required'}
        & C:\SuperHDDLab\signtool.exe sign /ph /fd SHA256 /sm /s My /sha1 $cert.Thumbprint (Join-Path $dir 'driver-v8.sys') *> (Join-Path $dir 'sign.log')
        if($LASTEXITCODE -or (Get-AuthenticodeSignature (Join-Path $dir 'driver-v8.sys')).Status -ne 'Valid'){throw 'Guest-only signature failed'}
        Add-Type -TypeDefinition $code
        $device=[SuperHDDHostPnp]::Device('S:')
        $node=@(Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq $device})
        if($node.Count -ne 1){throw 'Source devnode ambiguous'}
        & sc.exe create SuperHDDCacheV8 type= kernel start= demand binPath= (Join-Path $dir 'driver-v8.sys') *> (Join-Path $dir 'create-driver.log')
        if($LASTEXITCODE){throw 'Guest driver registration failed'}
        [SuperHDDHostPnp]::SetFilter($node[0].InstanceId,$device,$true)
        $key='HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
        $computer=Get-CimInstance Win32_ComputerSystem
        $backup=@{automatic=[bool]$computer.AutomaticManagedPagefile;paging=[string[]](Get-ItemPropertyValue $key PagingFiles);sourceNode=$node[0].InstanceId}
        $backup|ConvertTo-Json -Depth 4|Set-Content (Join-Path $dir 'pagefile-backup.json')
        $computer|Set-CimInstance -Property @{AutomaticManagedPagefile=$false}
        Set-ItemProperty $key PagingFiles -Value ([string[]]@('C:\pagefile.sys 0 0','S:\pagefile.sys 128 128'))
        [pscustomobject]@{signedDriverSha256=(Get-FileHash (Join-Path $dir 'driver-v8.sys')).Hash;sourceNode=$node[0].InstanceId;pagefileConfigured=$true;bootBefore=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime}
    };$pageChanged=$true;Save
    Step 'reboot-owned-vm-to-create-source-pagefile'
    Invoke-Command -Session $session -ScriptBlock {& shutdown.exe /r /t 3|Out-Null}
    Remove-PSSession $session;$session=$null;Start-Sleep -Seconds 10;$session=Connect-Owned
    $r.pagefile=Invoke-Command -Session $session -ArgumentList $run -ScriptBlock {
        param($run)
        if((Get-Volume -DriveLetter S).FileSystemLabel -ne ('SHDD-M-SOURCE-'+$run.Substring(0,8))){throw 'Source changed after boot'}
        $page=@(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys')
        if($page.Count -ne 1 -or $page[0].AllocatedBaseSize -ne 128){throw 'Actual source page file not created'}
        $page|Select-Object Name,AllocatedBaseSize,CurrentUsage
    };Save
    Step 'exercise-kernel-cache-with-source-pagefile-active'
    $r.probe=Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {
        param($dir)
        $ErrorActionPreference='Stop'
        $p=Start-Process -FilePath (Join-Path $dir 'probe.exe') -ArgumentList ('"'+(Join-Path $dir 'request.json')+'" "'+(Join-Path $dir 'proof.json')+'"') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $dir 'probe.log') -RedirectStandardError (Join-Path $dir 'probe-error.log')
        if(-not $p.WaitForExit(300000)){$p.Kill();throw 'Owned probe timed out'}
        if(-not(Test-Path (Join-Path $dir 'proof.json'))){throw 'Probe produced no proof; inspect guest error log'}
        Get-Content (Join-Path $dir 'proof.json') -Raw|ConvertFrom-Json
    }
    if(-not $r.probe.passed -or -not $r.probe.ownedFixturesRemoved){throw 'Transparent kernel/page-file coexistence proof failed'}
    $r.passed=$true
}catch{$r.error=$_.Exception.Message;$r.stack=$_.ScriptStackTrace}finally{
    if($session){
        try{Copy-Item -FromSession $session -LiteralPath $guestRun -Destination (Join-Path $hostRun 'guest-logs') -Recurse}catch{$r.copyError=$_.Exception.Message}
        if($pageChanged){
            try{
                Step 'restore-guest-pagefile-and-remove-owned-filter'
                Invoke-Command -Session $session -ArgumentList $guestRun -ScriptBlock {
                    param($dir)
                    $ErrorActionPreference='Stop'
                    $status=& (Join-Path $dir 'probe.exe') status|ConvertFrom-Json
                    if($LASTEXITCODE -or $status.cache.attached){throw 'Active kernel backing is preserved for diagnosis'}
                    $backup=Get-Content (Join-Path $dir 'pagefile-backup.json') -Raw|ConvertFrom-Json
                    Get-CimInstance Win32_ComputerSystem|Set-CimInstance -Property @{AutomaticManagedPagefile=[bool]$backup.automatic}
                    Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' PagingFiles -Value ([string[]]$backup.paging)
                    & shutdown.exe /r /t 3|Out-Null
                }
                Remove-PSSession $session;$session=$null;Start-Sleep -Seconds 10;$session=Connect-Owned
                $r.cleanup=Invoke-Command -Session $session -ArgumentList $guestRun,$helper -ScriptBlock {
                    param($dir,$code)
                    $ErrorActionPreference='Stop'
                    if(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys'){throw 'Source page file remains active'}
                    $backup=Get-Content (Join-Path $dir 'pagefile-backup.json') -Raw|ConvertFrom-Json
                    Add-Type -TypeDefinition $code
                    [SuperHDDHostPnp]::SetFilter($backup.sourceNode,[SuperHDDHostPnp]::Device('S:'),$false)
                    & pnputil.exe /restart-device $backup.sourceNode *> (Join-Path $dir 'remove-filter.log')
                    if($LASTEXITCODE){throw 'Owned filter removal failed'}
                    & sc.exe delete SuperHDDCacheV8 *> (Join-Path $dir 'delete-driver.log')
                    if($LASTEXITCODE){throw 'Owned driver deletion failed'}
                    [pscustomobject]@{pagefileRestored=$true;filterRemoved=$true;cacheDetached=$true}
                }
            }catch{$r.cleanupError=$_.Exception.Message;$r.passed=$false}
        }
        if($session){Remove-PSSession $session;$session=$null}
    }
    if($r.cleanup.filterRemoved){
        try{
            foreach($parent in $parents){
                $full=[IO.Path]::GetFullPath($parent.path);$expected=if($HostHddFiles -and $parent.letter -eq 'S'){'D:\SuperHDD-PagingTest-'+$run+'.vhdx'}else{Join-Path $hostRun ($parent.letter+'-parent.vhdx')}
                if($full -ne [IO.Path]::GetFullPath($expected) -or (Get-Item $full).CreationTimeUtc.ToFileTimeUtc() -ne $parent.creation){throw 'Owned parent identity changed'}
                $attachment=@($vm|Get-VMHardDiskDrive|Where-Object Path -eq $full)
                if($attachment.Count -ne 1){throw 'Owned VM attachment changed'}
                $attachment[0]|Remove-VMHardDiskDrive
                if(@(Get-VM|Get-VMHardDiskDrive|Where-Object Path -eq $full).Count){throw 'Parent remains attached'}
                Remove-Item -LiteralPath $full
            };$r.hostParentsRemoved=$true
        }catch{$r.hostCleanupError=$_.Exception.Message;$r.passed=$false}
    }
    $lock.Dispose()
    if($r.hostParentsRemoved){try{
        $session=Connect-Owned
        Invoke-Command -Session $session -ArgumentList $vm.Id.ToString() -ScriptBlock {
            param($id)
            if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $id){throw 'Wrong guest'}
            if(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys'){throw 'Source page file remains'}
            $refs=@(Get-PnpDevice -Class Volume|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data -contains 'SuperHDDCacheV8'})
            # A PnP driver marked for deletion can remain loaded until shutdown.
            # Every per-device enrollment must already be absent.
            if($refs.Count){throw 'Owned filter remains'}
            & shutdown.exe /s /t 3|Out-Null
        }
        Remove-PSSession $session;$session=$null
        for($i=0;$i -lt 60;$i++){$vm=Get-VM -Id $vm.Id;if($vm.State -eq 'Off'){break};Start-Sleep -Seconds 1}
        $active=@(Get-CimInstance Win32_SystemDriver|Where-Object {$_.State -eq 'Running' -and $_.Name -like 'SuperHDD*'}|Select-Object Name,State)
        $r.finalHost=@{passed=($vm.State -eq 'Off' -and $vm.MemoryAssigned -eq 0 -and $active.Count -eq 0);vmState=$vm.State.ToString();assignedVmMemoryBytes=$vm.MemoryAssigned;activeHostCacheDrivers=$active}
        if(-not $r.finalHost.passed){throw 'VM shutdown verification failed'}
    }catch{$r.shutdownError=$_.Exception.Message;$r.passed=$false}}
    $r.bootAfter=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $r.hostBootUnchanged=($r.bootBefore.machineSha256 -eq $r.bootAfter.machineSha256 -and $r.bootBefore.secureBootEnabled -eq $r.bootAfter.secureBootEnabled -and $r.bootBefore.codeIntegrityOptions -eq $r.bootAfter.codeIntegrityOptions)
    $r.passed=[bool]($r.passed -and $r.hostParentsRemoved -and $r.cleanup.pagefileRestored -and $r.hostBootUnchanged)
    $r.phase=if($r.passed){'completed'}else{'failed'};Save
}
if(-not $r.passed){throw 'Transparent v8 VM test failed; exact owned fixtures are preserved when recovery is uncertain.'}
