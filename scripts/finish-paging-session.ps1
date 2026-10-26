param([Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Run,[switch]$ShutdownOnly)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot;$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$record=Join-Path $root 'artifacts\paging-session.json';$r=Get-Content $record -Raw|ConvertFrom-Json -AsHashtable
if($r.runId -ne $Run -or -not $r.install.pagefileConfigured){throw 'Exact owned prepared run required'}
$vm=Get-VM -Id $r.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or $vm.Generation -ne 1 -or @($vm|Get-VMNetworkAdapter).Count){throw 'Only the owned isolated VM is accepted'}
$hostRun=Join-Path $lab ('paging-'+$Run);$guestRun='C:\SuperHDDLab\paging-'+$Run
$helper=(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'host-test-pnp.cs') -Raw).Replace('SuperHDDHostTest','SuperHDDCacheV8')
$credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
$session=$null
function Save {$r|ConvertTo-Json -Depth 15|Set-Content $record -Encoding utf8NoBOM}
if(-not $ShutdownOnly){
$lock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
try{
    $r.phase='restore-guest-pagefile';Save
    $session=New-PSSession -VMId $vm.Id -Credential $credential
    Invoke-Command -Session $session -ArgumentList $guestRun,$vm.Id.ToString() -ScriptBlock {
        param($dir,$id)
        $ErrorActionPreference='Stop'
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $id){throw 'Wrong VM'}
        $status=& (Join-Path $dir 'probe.exe') status|ConvertFrom-Json
        if($LASTEXITCODE -or $status.cache.attached){throw 'An active kernel backing is preserved'}
        $backup=Get-Content (Join-Path $dir 'pagefile-backup.json') -Raw|ConvertFrom-Json
        Get-CimInstance Win32_ComputerSystem|Set-CimInstance -Property @{AutomaticManagedPagefile=[bool]$backup.automatic}
        Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' PagingFiles -Value ([string[]]$backup.paging)
        & shutdown.exe /r /t 3|Out-Null
    }
    Remove-PSSession $session;$session=$null;Start-Sleep -Seconds 10
    for($i=0;$i -lt 60;$i++){try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 2}}
    if(-not $session){throw 'Owned VM did not return for cleanup'}
    $r.cleanup=Invoke-Command -Session $session -ArgumentList $guestRun,$helper,$vm.Id.ToString() -ScriptBlock {
        param($dir,$code,$id)
        $ErrorActionPreference='Stop'
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $id){throw 'Wrong VM'}
        if(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys'){throw 'Source page file remains active'}
        $backup=Get-Content (Join-Path $dir 'pagefile-backup.json') -Raw|ConvertFrom-Json
        $paging=[string[]](Get-ItemPropertyValue 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' PagingFiles)
        if(($paging -join '|') -ne ($backup.paging -join '|') -or (Get-CimInstance Win32_ComputerSystem).AutomaticManagedPagefile -ne $backup.automatic){throw 'Guest page-file policy was not restored'}
        Add-Type -TypeDefinition $code
        [SuperHDDHostPnp]::SetFilter($backup.sourceNode,[SuperHDDHostPnp]::Device('S:'),$false)
        & pnputil.exe /restart-device $backup.sourceNode *> (Join-Path $dir 'remove-filter.log')
        if($LASTEXITCODE){throw 'Owned source filter removal failed'}
        & sc.exe delete SuperHDDCacheV8 *> (Join-Path $dir 'delete-driver.log')
        if($LASTEXITCODE){throw 'Owned driver deletion failed'}
        [pscustomobject]@{pagefileRestored=$true;filterRemoved=$true;cacheDetached=$true}
    }
    Copy-Item -FromSession $session -LiteralPath $guestRun -Destination (Join-Path $hostRun 'final-guest-logs') -Recurse
    Remove-PSSession $session;$session=$null
    foreach($letter in @('S','K')){
        $path=[IO.Path]::GetFullPath((Join-Path $hostRun ($letter+'-parent.vhdx')))
        if([IO.Path]::GetDirectoryName($path) -ne [IO.Path]::GetFullPath($hostRun)){throw 'Parent escaped its exact owned run directory'}
        $vhd=Get-VHD -Path $path
        if($vhd.Size -ne 1GB -or $vhd.VhdType -ne 'Dynamic'){throw 'Owned 1 GiB VM parent required'}
        $attachment=@($vm|Get-VMHardDiskDrive|Where-Object Path -eq $path)
        if($attachment.Count -ne 1 -or $attachment[0].ControllerType -ne 'SCSI'){throw 'Exact owned VM attachment required'}
        $attachment[0]|Remove-VMHardDiskDrive
        if(@(Get-VM|Get-VMHardDiskDrive|Where-Object Path -eq $path).Count){throw 'Owned parent is still attached'}
        Remove-Item -LiteralPath $path
    }
    $r.hostParentsRemoved=$true
}catch{$r.cleanupError=$_.Exception.Message;$r.passed=$false;throw}finally{if($session){Remove-PSSession $session};$lock.Dispose();Save}
}elseif(-not $r.cleanup.pagefileRestored -or -not $r.cleanup.filterRemoved -or -not $r.hostParentsRemoved){throw 'Cleanup must be proven before shutdown-only completion'}
if($vm.State -eq 'Running'){
    $session=New-PSSession -VMId $vm.Id -Credential $credential
    try{Invoke-Command -Session $session -ArgumentList $vm.Id.ToString() -ScriptBlock {
        param($id)
        if($env:COMPUTERNAME -ne 'SHDD-LAB' -or (Get-Content C:\SuperHDDLab\vm-identity.txt -Raw).Trim() -ne $id){throw 'Wrong VM'}
        if(Get-CimInstance Win32_PageFileUsage|Where-Object Name -eq 'S:\pagefile.sys'){throw 'Owned source still has a page file'}
        $refs=@(Get-PnpDevice -Class Volume|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data -contains 'SuperHDDCacheV8'})
        if($refs.Count){throw 'A volume still references v8'}
        & shutdown.exe /s /t 3|Out-Null
    }}finally{Remove-PSSession $session;$session=$null}
    for($i=0;$i -lt 60;$i++){$vm=Get-VM -Id $vm.Id;if($vm.State -eq 'Off'){break};Start-Sleep -Seconds 1}
}
$active=@(Get-CimInstance Win32_SystemDriver|Where-Object {$_.State -eq 'Running' -and $_.Name -like 'SuperHDD*'}|Select-Object Name,State)
$r.finalHost=@{passed=($vm.State -eq 'Off' -and $vm.MemoryAssigned -eq 0 -and $active.Count -eq 0);vmState=$vm.State.ToString();assignedVmMemoryBytes=$vm.MemoryAssigned;activeHostCacheDrivers=$active}
$r.bootAfter=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$r.hostBootUnchanged=($r.bootBefore.machineSha256 -eq $r.bootAfter.machineSha256 -and $r.bootBefore.secureBootEnabled -eq $r.bootAfter.secureBootEnabled -and $r.bootBefore.codeIntegrityOptions -eq $r.bootAfter.codeIntegrityOptions)
$r.cleanupError=$null
$r.error=$null;$r.initialAttemptPassed=$false;$r.completedAfterProbeRetries=$true
$r.passed=[bool]($r.probe.passed -and $r.probe.ownedFixturesRemoved -and $r.cleanup.pagefileRestored -and $r.hostParentsRemoved -and $r.finalHost.passed -and $r.hostBootUnchanged)
$r.phase=if($r.passed){'completed'}else{'cleaned-with-failed-proof'};Save
