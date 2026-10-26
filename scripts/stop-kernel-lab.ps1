$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$lab=Join-Path $root 'artifacts\runtime\kernel-lab'
$runLock=[IO.File]::Open((Join-Path $lab 'hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
$session=$null
try{
    $configuration=Get-Content -LiteralPath (Join-Path $lab 'provisioning.json') -Raw|ConvertFrom-Json
    $benchmark=Get-Content -LiteralPath (Join-Path $lab 'hardware-benchmark.json') -Raw|ConvertFrom-Json
    $vm=Get-VM -Id $configuration.vmId
    if($vm.Name -ne 'SuperHDD-KernelLab-20261019' -or @($vm|Get-VMNetworkAdapter).Count -or $benchmark.phase -ne 'completed' -or -not $benchmark.hostHddFileRemoved -or (Test-Path -LiteralPath $benchmark.hddFile)){throw 'Lab shutdown ownership/cleanup check failed.'}
    $result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');vmId=$vm.Id.ToString();hostSecurityChanged=$false;benchmarkFileAbsent=$true}
    if($vm.State -eq 'Running'){
        $credential=Import-Clixml -LiteralPath (Join-Path $lab 'guest-credential.xml')
        $session=New-PSSession -VMId $vm.Id -Credential $credential
        $result.guest=Invoke-Command -Session $session -ScriptBlock {
            if($env:COMPUTERNAME -ne 'SHDD-LAB'){throw 'Wrong guest.'}
            & C:\SuperHDDLab\raw_lab.exe disable *> C:\SuperHDDLab\final-disable.log
            if($LASTEXITCODE){throw 'Cannot disable the lab cache before shutdown.'}
            $status=& C:\SuperHDDLab\raw_lab.exe status|ConvertFrom-Json
            if($LASTEXITCODE -or $status.cache.attached){throw 'Lab session remains attached.'}
            [pscustomobject]@{cacheDetached=$true;signature=(Get-AuthenticodeSignature C:\SuperHDDLab\driver.sys).Status.ToString();driverSha256=(Get-FileHash C:\SuperHDDLab\driver.sys).Hash}
            & shutdown.exe /s /t 5|Out-Null
        }
        Remove-PSSession $session;$session=$null
        for($attempt=0;$attempt -lt 60;$attempt++){
            $vm=Get-VM -Id $vm.Id
            if($vm.State -eq 'Off'){break}
            Start-Sleep -Seconds 1
        }
    }
    $result.vmState=$vm.State.ToString();$result.assignedVmMemoryBytes=$vm.MemoryAssigned
    $result.activeHostCacheDrivers=@(Get-CimInstance Win32_SystemDriver|Where-Object {$_.State -eq 'Running' -and ($_.Name+' '+$_.DisplayName) -match 'SuperHDD|PrimoCache|FancyCcV|rxfc'}|Select-Object Name,State)
    $os=Get-CimInstance Win32_OperatingSystem
    $result.hostRam=[ordered]@{totalBytes=[uint64]$os.TotalVisibleMemorySize*1024;availableBytes=[uint64]$os.FreePhysicalMemory*1024}
    $result.applicationRunning= [bool](Get-Process -Name SuperHDD -ErrorAction SilentlyContinue)
    $result.recentHostStorageErrors=@(Get-WinEvent -FilterHashtable @{LogName='System';StartTime=([DateTime]::Parse($benchmark.at).ToLocalTime());Id=7,11,51,129,153,157} -ErrorAction SilentlyContinue|Where-Object ProviderName -match '^(disk|storahci|stornvme|storport)$'|Select-Object TimeCreated,Id,ProviderName,Message)
    $result.passed=$result.vmState -eq 'Off' -and $result.assignedVmMemoryBytes -eq 0 -and $result.activeHostCacheDrivers.Count -eq 0
    $result|ConvertTo-Json -Depth 9|Set-Content -LiteralPath (Join-Path $lab 'final-host-state.json') -Encoding utf8NoBOM
    if(-not $result.passed){throw 'Lab did not stop cleanly; inspect final-host-state.json.'}
}finally{
    if($session){Remove-PSSession $session -ErrorAction SilentlyContinue}
    $runLock.Dispose()
}
