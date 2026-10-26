param([Parameter(Mandatory=$true)][string]$RunId)
$ErrorActionPreference='Stop'
if($RunId -notmatch '^[a-f0-9]{32}$'){throw 'Use an exact run ID.'}
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
trap {
    @{at=[DateTime]::UtcNow.ToString('o');runId=$RunId;passed=$false;error=$_.Exception.Message;stack=$_.ScriptStackTrace}|
        ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $lab ('direct-cleanup-'+$RunId+'.json')) -Encoding utf8NoBOM
    exit 1
}
$proof=Get-Content (Join-Path $lab 'direct-session.json') -Raw|ConvertFrom-Json
if($proof.runId -ne $RunId -or -not $proof.probe.passed -or -not $proof.probe.ownedFixturesRemoved -or $proof.cleanupError -notin 'V5 driver unload failed.','V5 driver remains registered.'){throw 'This recovery handles only the recorded successful probe/driver-registration cleanup failure.'}
$vm=Get-VM -Id $proof.vmId
if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or @($vm|Get-VMNetworkAdapter).Count){throw 'Wrong isolated VM.'}
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$RunId;passed=$false}
$lock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$session=$null
try{
    if($vm.State -eq 'Off'){Start-VM -VM $vm|Out-Null}
    $credential=Import-Clixml (Join-Path $lab 'guest-credential.xml')
    for($attempt=0;$attempt -lt 40;$attempt++){try{$session=New-PSSession -VMId $vm.Id -Credential $credential -ErrorAction Stop;break}catch{Start-Sleep -Seconds 3}}
    if(-not $session){throw 'Guest unavailable.'}
    $result.guest=Invoke-Command -Session $session -ArgumentList $RunId,$proof.source.node -ScriptBlock {
        param($RunId,$Node)
        $ErrorActionPreference='Stop'
        if($env:COMPUTERNAME -ne 'SHDD-LAB'){throw 'Wrong guest.'}
        $filters=(Get-PnpDeviceProperty -InstanceId $Node -KeyName DEVPKEY_Device_UpperFilters -ErrorAction SilentlyContinue).Data
        if($filters -contains 'SuperHDDCacheV5'){throw 'Owned filter still enrolled; preserve disks.'}
        if(Get-CimInstance Win32_Service -Filter "Name='SuperHDDService'"){throw 'SCM owner remains.'}
        if(Test-Path 'C:\ProgramData\SuperHDDDirect'){throw 'Protected state remains.'}
        $driver=Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheV5'"
        if($driver){
            $expected='C:\Program Files\SuperHDDLab\Managed-'+$RunId+'\driver-v5.sys'
            if($driver.PathName.Trim('"').Replace('\??\','') -ne $expected){throw 'Driver registration belongs to another image.'}
            & sc.exe delete SuperHDDCacheV5|Out-Null
            if($LASTEXITCODE){throw 'Cannot remove owned driver registration.'}
        }
        [pscustomobject]@{sourceFilterAbsent=$true;ownerServiceAbsent=$true;protectedStateAbsent=$true;driverRegistrationAbsent=(-not(Get-CimInstance Win32_SystemDriver -Filter "Name='SuperHDDCacheV5'"))}
    }
    if(-not $result.guest.driverRegistrationAbsent){throw 'Driver registration remains.'}
    # The standard shutdown verifier observes the separate frozen v4 endpoint.
    # Restore only its already owned default lab volumes before that verifier.
    Invoke-Command -Session $session -FilePath (Join-Path $root 'scripts\kernel-lab-guest.ps1') -ArgumentList 'ResumeVolumes',$vm.Id.ToString()|Out-Null
}finally{if($session){Remove-PSSession $session};$lock.Dispose()}
& (Join-Path $root 'scripts\stop-kernel-lab.ps1')
$vm=Get-VM -Id $proof.vmId
if($vm.State -ne 'Off' -or $vm.MemoryAssigned -ne 0){throw 'Guest must be fully off before parent removal.'}
$runRoot=[IO.Path]::GetFullPath((Join-Path $lab ('managed-'+$RunId)))
foreach($parent in $proof.parents){
    $absolute=[IO.Path]::GetFullPath($parent.path)
    if([IO.Path]::GetDirectoryName($absolute) -ne $runRoot -or [IO.Path]::GetFileName($absolute) -notin 'source-parent.vhdx','cache-parent.vhdx'){throw 'Cleanup path outside the exact prior run.'}
    $attachments=@($vm|Get-VMHardDiskDrive|Where-Object Path -eq $absolute)
    if($attachments.Count -ne 1){throw 'Prior attachment is missing or ambiguous.'}
    $attachments[0]|Remove-VMHardDiskDrive
    if(@(Get-VM|Get-VMHardDiskDrive|Where-Object Path -eq $absolute).Count){throw 'Parent still attached.'}
    Remove-Item -LiteralPath $absolute
}
$result.parentsRemoved=$true;$result.vmOff=$true;$result.passed=$true
$result|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $lab ('direct-cleanup-'+$RunId+'.json')) -Encoding utf8NoBOM
