# Recover only an already recorded disposable VHDX handoff. Never enroll D itself.
param([Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Run,[int]$SupersededProcess=0)
$ErrorActionPreference='Stop'
if($SupersededProcess){
    $previousProcess=Get-CimInstance Win32_Process -Filter ('ProcessId='+$SupersededProcess)
    if($previousProcess -and ($previousProcess.ExecutablePath -ne (Join-Path $PSHOME 'pwsh.exe') -or $previousProcess.CommandLine -notmatch [regex]::Escape($PSCommandPath) -or $previousProcess.CommandLine -notmatch [regex]::Escape($Run) -or $SupersededProcess -eq $PID)){throw 'Only the superseded instance of this exact fixture recovery may be stopped.'}
    if($previousProcess){Stop-Process -Id $SupersededProcess -Force}
}
$root=Split-Path -Parent $PSScriptRoot
$record=Join-Path $root 'artifacts\iscsi-local-latest.json'
$saved=Get-Content -LiteralPath $record -Raw|ConvertFrom-Json
if($saved.runId -ne $Run -or -not $saved.ownedVhdHandoffRequested -or -not $saved.nativeInitiatorRequested){throw 'Only the exact recorded native VHDX handoff may be recovered.'}
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$Run)
$source='D:\SuperHDD-handoff-'+$Run+'.vhdx'
$allowedCache=@((Join-Path $runRoot 'ssd-handoff-cache.bin'),(Join-Path $runRoot 'private-ssd\payload.bin'))
if($source -cne $saved.sourcePath -or $saved.cachePath -cnotin $allowedCache){throw 'Recorded paths differ from the owned fixture paths.'}
$cache=$saved.cachePath
$native=Join-Path $root 'artifacts\iscsi-probe\native-initiator.exe'
$exe=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
foreach($entry in $saved.build.binaries){if((Get-FileHash -LiteralPath $entry.Path).Hash -ne $entry.Hash){throw 'Fixture recovery binary changed.'}}
$iqn='iqn.2026-10.local.superhdd:probe-'+$Run
$result=[ordered]@{runId=$Run;passed=$false;error=$null;scope='Recovery of this owned VHDX fixture only'}
$out=Join-Path $root ('artifacts\iscsi-handoff-recovery-'+$Run+'.json')
function Save {$result|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $out -Encoding utf8NoBOM}
function Native([string]$operation,[string]$volume){
    $arguments=@($operation,$Run);if($volume){$arguments+=$volume}
    $output=& $native @arguments
    if($LASTEXITCODE){throw ('Native owned-fixture operation failed: '+$operation)}
    $output|ConvertFrom-Json
}
function Source {
    $vhd=Get-VHD -Path $source
    $marker=Get-Content -LiteralPath (Join-Path $runRoot 'handoff-source-owner.json') -Raw|ConvertFrom-Json
    if(-not $vhd.Attached -or $vhd.Size -ne 128MB -or $marker.runId -ne $Run -or (Get-Item -LiteralPath $source).CreationTimeUtc.ToFileTimeUtc() -ne $marker.created){throw 'Owned source dependency changed.'}
    $disk=Get-Disk -Number $vhd.DiskNumber
    if($disk.IsBoot -or $disk.IsSystem -or $disk.BusType -ne 'File Backed Virtual' -or $disk.Guid -ne $saved.originalSourceDisk.Guid -or $disk.Size -ne 128MB){throw 'Only the exact original virtual source may change state.'}
    $disk
}
function Mount-OwnedRecoveryPartition($disk){
    $parts=@(Get-Partition -DiskNumber $disk.Number|Where-Object {$_.Guid -eq $saved.originalSourcePartition.Guid -and $_.Offset -eq $saved.originalSourcePartition.Offset -and $_.Size -eq $saved.originalSourcePartition.Size})
    if($parts.Count -ne 1){throw 'Owned data partition changed.'}
    $other=Get-Partition -DriveLetter S -ErrorAction SilentlyContinue
    if($other -and ($other.DiskNumber -ne $disk.Number -or $other.Guid -ne $parts[0].Guid)){throw 'S belongs to a different volume.'}
    if($parts[0].DriveLetter -ne 'S'){$parts[0]|Set-Partition -NewDriveLetter S}
}
function Oracle([string]$tag){
    $report=Join-Path $runRoot ($tag+'.json')
    & $exe client read-all-mutated 'S:\fixture.bin' $report
    if($LASTEXITCODE){throw 'Full recovered fixture byte oracle failed.'}
    if((Get-Content -LiteralPath 'S:\preserved.txt' -Raw) -cne ('Existing file '+$Run)){throw 'Other fixture data changed.'}
    Get-Content -LiteralPath $report -Raw|ConvertFrom-Json
}
$lock=$null
try {
    $lock=[IO.File]::Open((Join-Path $root 'artifacts\runtime\kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    $result.phase='verify-owned-source';Save
    $null=Source
    $servers=@(Get-CimInstance Win32_Process -Filter "Name='iscsi-probe.exe'"|Where-Object {$_.ExecutablePath -eq $exe -and $_.CommandLine -match ('server-device(-reopen)?-512\s+'+[regex]::Escape($Run)+'\s+\d+\s*$')})
    if($servers.Count -gt 1){throw 'Ambiguous owned worker; preserving it.'}
    $sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq $iqn)
    if($sessions.Count -gt 1){throw 'Ambiguous owned session; preserving it.'}
    if($sessions.Count){
        $disks=@($sessions|Get-Disk)
        if($disks.Count -ne 1 -or $disks[0].BusType -ne 'iSCSI' -or $disks[0].SerialNumber.Trim() -ne $Run -or $disks[0].Size -ne 128MB -or $disks[0].IsBoot -or $disks[0].IsSystem){throw 'Owned target disk is not positively identified.'}
        if($servers.Count -ne 1){throw 'Expected one live worker for fixture verification.'}
        $result.phase='verify-target-full-payload';Save
        $result.targetOnline=Native restore-target
        Mount-OwnedRecoveryPartition (Get-Disk -Number $disks[0].Number)
        $result.targetOracle=Oracle recovery-target-full
        $result.targetOffline=Native lock-target 'S:'
        $result.disconnected=Native disconnect
    }
    if($servers.Count){
        $result.phase='stop-drained-worker';Save
        $token=[Guid]::NewGuid().ToString('N')
        Set-Content -LiteralPath (Join-Path $runRoot 'command') -Value ($token+' 5') -Encoding ascii
        $deadline=[DateTime]::UtcNow.AddSeconds(20)
        while([DateTime]::UtcNow -lt $deadline){
            $ack=Join-Path $runRoot 'ack'
            if((Test-Path -LiteralPath $ack) -and (Get-Content -LiteralPath $ack -Raw).Trim() -eq ($token+' 0')){break}
            Start-Sleep -Milliseconds 100
        }
        if(-not(Test-Path -LiteralPath $ack) -or (Get-Content -LiteralPath $ack -Raw).Trim() -ne ($token+' 0')){throw 'Owned worker did not acknowledge drained shutdown.'}
        $process=Get-Process -Id $servers[0].ProcessId -ErrorAction SilentlyContinue
        if($process -and -not $process.WaitForExit(15000)){throw 'Owned worker did not close its source handle.'}
    }
    $result.sourceOnline=Native restore-source
    $result.phase='verify-original-full-payload';Save
    Mount-OwnedRecoveryPartition (Source)
    $result.sourceOracle=Oracle recovery-source-full
    (Source)|Set-Disk -IsOffline $true
    $result.phase='detach-owned-fixture';Save
    Dismount-VHD -Path $source
    if((Get-VHD -Path $source).Attached){throw 'Owned source remains attached.'}
    $result.sourceSha256=(Get-FileHash -LiteralPath $source).Hash
    foreach($path in @($source,$cache)){if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}}
    if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Other initiator resources remain; preserving service.'}
    if($saved.initiatorBefore.status -eq 'Stopped'){Stop-Service MSiSCSI}
    Set-Service MSiSCSI -StartupType $saved.initiatorBefore.startType
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootUnchanged=($after.machineSha256 -eq $saved.bootBefore.machineSha256 -and $after.codeIntegrityOptions -eq $saved.bootBefore.codeIntegrityOptions -and $after.secureBootEnabled -eq $saved.bootBefore.secureBootEnabled)
    $result.sourceRemoved=-not(Test-Path -LiteralPath $source)
    $result.cacheRemoved=-not(Test-Path -LiteralPath $cache)
    $service=Get-Service MSiSCSI
    $result.initiatorRestored=([string]$service.Status -eq $saved.initiatorBefore.status -and [string]$service.StartType -eq $saved.initiatorBefore.startType)
    $result.passed=[bool]($result.sourceRemoved -and $result.cacheRemoved -and $result.initiatorRestored -and $result.bootUnchanged)
    $result.phase='finished'
}catch{$result.error=$_.Exception.Message}finally{Save;if($lock){$lock.Dispose()}}
$result|ConvertTo-Json -Depth 8
if(-not $result.passed){throw 'Owned VHDX fixture recovery did not pass.'}
