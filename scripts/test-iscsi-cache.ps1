# Disposable 128 MiB file on D and 32 MiB cache on M. No existing disk enrollment.
# Temporarily starts the built-in Microsoft initiator; restores its prior state.
param([switch]$Recovery,[switch]$Handoff,[switch]$NativeInitiator,[ValidateSet(512,4096)][int]$SectorBytes=4096)
$ErrorActionPreference='Stop'
if($Handoff -and $SectorBytes -ne 512){throw 'Owned VHDX handoff requires 512-byte sectors.'}
$root=Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'remove-private-iscsi-portal.ps1')
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$run)
$record=Join-Path $root 'artifacts\iscsi-local-latest.json'
$exe=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
$initiatorExe=Join-Path $root 'artifacts\iscsi-probe\native-initiator.exe'
$source='D:\SuperHDD-iSCSI-'+$run+'.img'
$cache=Join-Path $runRoot 'ssd-cache.bin'
if($Handoff){$source='D:\SuperHDD-handoff-'+$run+'.vhdx';$cache=Join-Path $runRoot 'ssd-handoff-cache.bin'}
if($Handoff -and $NativeInitiator){$cache=Join-Path $runRoot 'private-ssd\payload.bin'}
$recoveredCache=Join-Path $runRoot 'ssd-cache-recovered.bin'
$serverMode=if($SectorBytes -eq 512){'server-512'}else{'server'}
$reopenMode=if($SectorBytes -eq 512){'server-reopen-512'}else{'server-reopen'}
$iqn='iqn.2026-10.local.superhdd:probe-'+$run
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Administrator required for the temporary Microsoft initiator session.'}
if(-not(Test-Path -LiteralPath $exe)){throw 'Build the file-only probe first.'}
if($NativeInitiator -and -not(Test-Path -LiteralPath $initiatorExe)){throw 'Build the native initiator controller first.'}
$built=Get-Content -LiteralPath (Join-Path (Split-Path $exe) 'build.json') -Raw|ConvertFrom-Json
foreach($item in @($built.sources)+@($built.binaries)){if((Get-FileHash -LiteralPath $item.Path).Hash -ne $item.Hash){throw 'Probe source/binary differs from its build manifest; rebuild before testing.'}}
if(Get-PSDrive -Name S -ErrorAction SilentlyContinue){throw 'S is in use; existing mounts are preserved.'}
if(Get-NetTCPConnection -State Listen -LocalPort 33260 -ErrorAction SilentlyContinue){throw 'Private test port is in use.'}
$hdd=Get-Partition -DriveLetter D|Get-Disk
$ssd=Get-Partition -DriveLetter M|Get-Disk
if($hdd.FriendlyName -notmatch 'HUA722020ALA331' -or $hdd.IsBoot -or $hdd.IsSystem -or $ssd.FriendlyName -notmatch '980 PRO' -or $hdd.UniqueId -eq $ssd.UniqueId){throw 'Expected separate D HDD and M SSD unavailable.'}
if((Get-Volume D).SizeRemaining -lt 1GB -or (Get-Volume M).SizeRemaining -lt 1GB){throw 'Insufficient free space for disposable files.'}
$boot=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$service=Get-Service MSiSCSI
$previous=[pscustomobject]@{status=[string]$service.Status;startType=[string]$service.StartType}
New-Item -ItemType Directory -Path $runRoot | Out-Null
$result=[ordered]@{at=[DateTime]::UtcNow.ToString('o');runId=$run;passed=$false;phase='starting';scope='Disposable file-backed block-cache proof only; existing D volume is not intercepted';bootBefore=$boot;initiatorBefore=$previous;newKernelDriverInstalled=$false;existingVolumesRemapped=$false;sourcePath=$source;cachePath=$cache;recoveryRequested=[bool]$Recovery;error=$null}
$result.ownedVhdHandoffRequested=[bool]$Handoff
$result.nativeInitiatorRequested=[bool]$NativeInitiator
$result.build=$built
$result.binaries=@('iscsi-probe.exe','superhdd_iscsi_probe.dll','ISCSI.dll','DiskAccessLibrary.dll'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path (Split-Path $exe) $_)|Select-Object Path,Hash})
if($NativeInitiator){$result.binaries+=Get-FileHash -LiteralPath $initiatorExe|Select-Object Path,Hash}
$result.sourceHashes=@('crates\cache-core\src\lib.rs','tools\iscsi-cache-probe\src\lib.rs','tools\iscsi-cache-probe\src\device.rs','tools\iscsi-cache-probe\Probe.cs','scripts\test-iscsi-cache.ps1','scripts\remove-private-iscsi-portal.ps1'|ForEach-Object{Get-FileHash -LiteralPath (Join-Path $root $_)|Select-Object Path,Hash})
if($NativeInitiator){$result.sourceHashes+=Get-FileHash -LiteralPath (Join-Path $root 'crates\storage-control\src\iscsi.rs')|Select-Object Path,Hash}
function Save { $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $record -Encoding utf8NoBOM }
function Phase([string]$name) { $result.phase=$name;Save }
function Wait-For([scriptblock]$action,[int]$seconds=20) {
    $deadline=[DateTime]::UtcNow.AddSeconds($seconds)
    do {$value=& $action;if($value){return $value};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
    throw 'Timed out waiting for private iSCSI proof.'
}
function Control([int]$operation) {
    $token=[Guid]::NewGuid().ToString('N')
    Set-Content -LiteralPath (Join-Path $runRoot 'command.tmp') -Value ($token+' '+$operation) -NoNewline
    Move-Item -LiteralPath (Join-Path $runRoot 'command.tmp') -Destination (Join-Path $runRoot 'command')
    $null=Wait-For {if(Test-Path -LiteralPath (Join-Path $runRoot 'ack')){(Get-Content -LiteralPath (Join-Path $runRoot 'ack') -Raw) -eq ($token+' 0')}}
    if($operation -ne 5){return Get-Content -LiteralPath (Join-Path $runRoot 'stats.json') -Raw|ConvertFrom-Json}
}
function Client([string]$mode,[string]$tag,[switch]$RetryTransient) {
    $report=Join-Path $runRoot ($tag+'.json')
    $deadline=[DateTime]::UtcNow.AddSeconds(60)
    $attempt=0
    do {
        $attempt++
        $logTag=$tag+'-'+$attempt
        $client=Start-Process -FilePath $exe -ArgumentList @('client',$mode,'S:\fixture.bin',('"'+$report+'"')) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot ($logTag+'.log')) -RedirectStandardError (Join-Path $runRoot ($logTag+'-error.log')) -PassThru
        $remaining=[Math]::Max(1,[int]($deadline-[DateTime]::UtcNow).TotalMilliseconds)
        if(-not $client.WaitForExit($remaining)){$client.Kill();$null=$client.WaitForExit(5000);throw ('Native fixture check timed out: '+$tag)}
        $value=Get-Content -LiteralPath $report -Raw|ConvertFrom-Json
        if(-not $client.ExitCode){return $value}
        # Retry only explicit native device-arrival errors, never byte mismatches.
        if(-not $RetryTransient -or $value.win32Error -notin @(2,21,55,433,1167) -or [DateTime]::UtcNow -ge $deadline){throw ('Native unbuffered fixture check failed: '+$logTag)}
        Start-Sleep -Milliseconds 500
    }while($true)
}
function Owned-Disk {
    $sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq $iqn)
    if($sessions.Count -ne 1){throw 'Private target session identity mismatch.'}
    $disks=@(Get-Disk -iSCSISession $sessions[0])
    if($disks.Count -ne 1 -or $disks[0].BusType -ne 'iSCSI' -or $disks[0].Size -ne 128MB -or $disks[0].SerialNumber.Trim() -ne $run -or $disks[0].Number -in @($hdd.Number,$ssd.Number) -or $disks[0].IsBoot -or $disks[0].IsSystem){throw 'Disposable disk identity mismatch; no disk mutation allowed.'}
    $disks[0]
}
function Owned-Source {
    $vhd=Get-VHD -Path $source
    $metadata=Get-Item -LiteralPath $source
    if(-not $sourceCreated -or -not $vhd.Attached -or $vhd.Size -ne 128MB -or $metadata.CreationTimeUtc.ToFileTimeUtc() -ne $sourceCreatedTime){throw 'Owned source VHDX identity changed.'}
    $disk=Get-Disk -Number $vhd.DiskNumber
    if($disk.BusType -ne 'File Backed Virtual' -or $disk.Size -ne 128MB -or $disk.IsBoot -or $disk.IsSystem -or $disk.Number -in @($hdd.Number,$ssd.Number)){throw 'Only the owned VHDX source may change state.'}
    if($sourceGuid -and $disk.Guid -ne $sourceGuid){throw 'Owned source GPT identity changed; preserve it.'}
    $disk
}
function Mount-OwnedPartition($disk) {
    $parts=@(Get-Partition -DiskNumber $disk.Number|Where-Object {$_.Guid -eq $sourcePartition.Guid -and $_.Offset -eq $sourcePartition.Offset -and $_.Size -eq $sourcePartition.Size})
    if($parts.Count -ne 1){throw 'Existing source partition changed; no mount change allowed.'}
    if($parts[0].DriveLetter -ne 'S'){
        if(Get-Partition -DriveLetter S -ErrorAction SilentlyContinue){throw 'S belongs to another partition.'}
        $parts[0]|Set-Partition -NewDriveLetter S
    }
    $null=Wait-For {Test-Path -LiteralPath 'S:\fixture.bin'}
    $mounted=Get-Partition -DriveLetter S
    if($mounted.DiskNumber -ne $disk.Number -or $mounted.Guid -ne $sourcePartition.Guid){throw 'Private fixture mount identity mismatch.'}
    $mounted
}
function Watch-Fixture([string]$tag,[switch]$RetryTransient) {
    $map=Client extents $tag -RetryTransient:$RetryTransient
    $ranges=@();$total=0L
    foreach($extent in $map.extents){
        $start=[long]$partition.Offset+[long]$extent.volumeOffset
        $length=[long]$extent.length
        if($start -lt $partition.Offset -or $length -le 0 -or $start+$length -gt $partition.Offset+$partition.Size){throw 'Fixture extent outside owned test partition.'}
        $ranges+=,[ordered]@{offset=$start;length=$length}
        $total+=$length
    }
    if($ranges.Count -eq 0 -or $ranges.Count -gt 128 -or $total -ne 16MB){throw 'Fixture allocation changed.'}
    $lines=@($ranges|ForEach-Object{([long]$_.offset).ToString([Globalization.CultureInfo]::InvariantCulture)+' '+([long]$_.length).ToString([Globalization.CultureInfo]::InvariantCulture)})
    Set-Content -LiteralPath (Join-Path $runRoot 'watch-ranges') -Value $lines -Encoding utf8NoBOM
    $null=Control 6
    $map|Add-Member NoteProperty sourceRanges $ranges
    $map
}
function Traffic-Delta($before,$after) {
    [ordered]@{
        hits=$after.watchedStatistics.hits-$before.watchedStatistics.hits
        ssdReadBytes=$after.watchedStatistics.ssdReadBytes-$before.watchedStatistics.ssdReadBytes
        sourceReadBytes=$after.watchedStatistics.sourceReadBytes-$before.watchedStatistics.sourceReadBytes
        scope='Cache traffic caused by requests overlapping the actual fixture extents'
        globalSourceReadBytes=$after.statistics.sourceReadBytes-$before.statistics.sourceReadBytes
    }
}
function Native-Initiator([string]$operation,[string]$volume){
    $nativeArgs=@($operation,$run)
    if($volume){$nativeArgs+=$volume}
    $json=& $initiatorExe @nativeArgs 2> (Join-Path $runRoot ('native-'+$operation+'-error.log'))
    if($LASTEXITCODE){throw ('Native initiator failed: '+$operation)}
    $json|ConvertFrom-Json
}
$owned=$null;$portalCreated=$false;$connected=$false;$lock=$null
$sourceCreated=$false;$sourceCreatedTime=$null;$sourceGuid=$null;$sourcePartition=$null;$sourceFixtureReady=$false;$mutationCompleted=$false
try {
    Save
    $lock=[IO.File]::Open((Join-Path $root 'artifacts\runtime\kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    $serverArgs=@($serverMode,$run)
    if($NativeInitiator){
        $result.existingDReadOnlyInspection=Native-Initiator inspect 'D:'
        try{$result.existingDEncryption=Get-BitLockerVolume -MountPoint 'D:'|Select-Object MountPoint,VolumeStatus,EncryptionPercentage,ProtectionStatus,LockStatus}catch{$result.existingDEncryption=[ordered]@{verified=$false;error=$_.Exception.Message}}
        Save
    }
    if($NativeInitiator){Set-Content -LiteralPath (Join-Path $runRoot 'kernel-initiator-only') -Value 'Windows kernel initiator only' -Encoding ascii}
    if($Handoff){
        if($NativeInitiator){
            if($service.StartType -eq 'Disabled'){Set-Service MSiSCSI -StartupType Manual}
            Start-Service MSiSCSI
        }
        Phase 'create-owned-source-with-existing-data'
        $null=New-VHD -Path $source -Fixed -SizeBytes 128MB -LogicalSectorSizeBytes 512 -PhysicalSectorSizeBytes 512
        $sourceCreated=$true
        $sourceCreatedTime=(Get-Item -LiteralPath $source).CreationTimeUtc.ToFileTimeUtc()
        $null=Mount-VHD -Path $source -NoDriveLetter
        $disk=Owned-Source
        $disk|Set-Disk -IsOffline $false
        $disk|Set-Disk -IsReadOnly $false
        $null=Initialize-Disk -Number $disk.Number -PartitionStyle GPT
        $sourceGuid=(Get-Disk -Number $disk.Number).Guid
        $sourcePartition=New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter S
        $null=Format-Volume -DriveLetter S -FileSystem NTFS -AllocationUnitSize 4096 -NewFileSystemLabel 'SuperHDD-Existing' -Confirm:$false -Force
        $result.seed=Client seed source-seed
        [IO.File]::WriteAllText('S:\preserved.txt',('Existing file '+$run))
        $result.sourceFixtureHash=(Get-FileHash -LiteralPath 'S:\fixture.bin').Hash
        $sourceFixtureReady=$true
        $result.originalSourceDisk=(Owned-Source)|Select-Object Number,Guid,BusType,Size,LogicalSectorSize
        $result.originalSourcePartition=$sourcePartition|Select-Object PartitionNumber,Guid,Offset,Size
        [ordered]@{runId=$run;created=$sourceCreatedTime;sourceBytes=128MB}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $runRoot 'handoff-source-owner.json') -Encoding utf8NoBOM
        Phase 'offline-only-owned-source'
        if($NativeInitiator){$result.nativeSourceOffline=Native-Initiator lock-source 'S:'}else{(Owned-Source)|Set-Disk -IsOffline $true}
        $sourceDisk=Owned-Source
        if(-not $sourceDisk.IsOffline){throw 'Owned source did not become offline.'}
        $serverArgs=@('server-device-512',$run,[string]$sourceDisk.Number)
    }
    $owned=Start-Process -FilePath $exe -ArgumentList $serverArgs -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'server.log') -RedirectStandardError (Join-Path $runRoot 'server-error.log') -PassThru
    $null=Wait-For {if($owned.HasExited){throw 'File-only target exited before ready.'};Test-Path -LiteralPath (Join-Path $runRoot 'ready')}
    if((Get-Content -LiteralPath (Join-Path $runRoot 'ready') -Raw) -ne $iqn){throw 'Unexpected target name.'}
    Phase 'connect-localhost'
    if($NativeInitiator){
        & $exe protocol-denied $run *> (Join-Path $runRoot 'untrusted-peer.log')
        if($LASTEXITCODE){throw 'Protected target did not reject a user-process initiator.'}
        $result.userProcessInitiatorRefused=$true
    }
    if($service.StartType -eq 'Disabled'){Set-Service MSiSCSI -StartupType Manual}
    Start-Service MSiSCSI
    if($NativeInitiator){
        $result.nativeConnect=Native-Initiator connect
    }else{
        if(Get-IscsiTargetPortal|Where-Object {$_.TargetPortalAddress -eq '127.0.0.1' -and $_.TargetPortalPortNumber -eq 33260}){throw 'Existing private portal preserved.'}
        $null=New-IscsiTargetPortal -TargetPortalAddress '127.0.0.1' -TargetPortalPortNumber 33260
        $portalCreated=$true
        $null=Connect-IscsiTarget -NodeAddress $iqn -TargetPortalAddress '127.0.0.1' -TargetPortalPortNumber 33260 -IsPersistent $false -IsMultipathEnabled $false
    }
    $connected=$true
    $disk=Wait-For {try {Owned-Disk}catch {return $null}}
    if($NativeInitiator -and $result.nativeConnect.devices[0].diskNumber -ne $disk.Number){throw 'Native SDK disk mapping differs from the guarded Windows session disk.'}
    $expectedStyle=if($Handoff){'GPT'}else{'RAW'}
    if($disk.PartitionStyle -ne $expectedStyle -or $disk.LogicalSectorSize -ne $SectorBytes){throw 'Disposable disk partition style/sector size mismatch.'}
    $result.disposableDisk=$disk|Select-Object Number,SerialNumber,BusType,Size,LogicalSectorSize,UniqueId
    Phase $(if($Handoff){'mount-existing-data-through-cache-without-format'}else{'format-owned-disposable-disk'})
    if($Handoff -and $NativeInitiator){$result.nativeTargetOnline=Native-Initiator restore-target}else{$disk|Set-Disk -IsOffline $false}
    $disk|Set-Disk -IsReadOnly $false
    if($Handoff){
        $partition=Mount-OwnedPartition $disk
        if((Get-FileHash -LiteralPath 'S:\fixture.bin').Hash -ne $result.sourceFixtureHash -or (Get-Content -LiteralPath 'S:\preserved.txt' -Raw) -cne ('Existing file '+$run)){throw 'Existing data changed during handoff.'}
        $result.existingDataPreservedOnHandoff=$true
        $result.cacheSideFormattingPerformed=$false
    }else{
        $null=Initialize-Disk -Number $disk.Number -PartitionStyle GPT
        $partition=New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter S
        $null=Format-Volume -DriveLetter S -FileSystem NTFS -AllocationUnitSize 4096 -NewFileSystemLabel 'SuperHDD-Probe' -Confirm:$false -Force
    }
    Phase 'native-ntfs-write-read'
    if(-not $Handoff){$result.seed=Client seed seed}
    $result.fixtureMap=Watch-Fixture fixture-extents
    $result.cleared=Control 1
    $result.cold=Client read cold
    $cold=Control 0
    $result.warm=Client read warm
    $warm=Control 0
    $result.warmDelta=Traffic-Delta $cold $warm
    if($result.warmDelta.hits -lt 96 -or $result.warmDelta.ssdReadBytes -lt 6MB -or $result.warmDelta.sourceReadBytes -ne 0){throw 'Warm reads did not stay in the SSD cache.'}
    $paused=Control 2
    $result.pausedRead=Client read paused
    $afterPause=Control 0
    if($afterPause.watchedStatistics.sourceReadBytes -le $paused.watchedStatistics.sourceReadBytes){throw 'Pause did not bypass fixture reads to HDD.'}
    $null=Control 3
    $result.mutation=Client mutate mutation
    $mutationCompleted=$true
    $result.finalStats=Control 0
    if($result.finalStats.statistics.invalidations -le 0){throw 'No observed write invalidation.'}
    if($Recovery){
        if($Handoff){$recoverySourceNumber=(Owned-Source).Number}
        Phase 'interrupt-owned-target-process'
        $result.interruptedPid=$owned.Id
        # Only this test's child is terminated, after all fixture writes completed.
        # The Windows initiator remains connected: this exercises its reconnect.
        $owned.Kill()
        if(-not $owned.WaitForExit(5000)){throw 'Owned disposable target did not exit.'}
        if($Handoff){$beforeReuse=Get-Item -LiteralPath $cache;$result.reusedCacheBefore=[ordered]@{created=$beforeReuse.CreationTimeUtc.ToFileTimeUtc();length=$beforeReuse.Length}}
        Remove-Item -LiteralPath (Join-Path $runRoot 'ready')
        $reopenArgs=if($Handoff){@('server-device-reopen-512',$run,[string]$recoverySourceNumber)}else{@($reopenMode,$run)}
        $owned=Start-Process -FilePath $exe -ArgumentList $reopenArgs -WorkingDirectory (Split-Path $exe) -WindowStyle Hidden -RedirectStandardOutput (Join-Path $runRoot 'reopened-server.log') -RedirectStandardError (Join-Path $runRoot 'reopened-server-error.log') -PassThru
        $null=Wait-For {if($owned.HasExited){throw 'Reopening the owned source failed.'};Test-Path -LiteralPath (Join-Path $runRoot 'ready')}
        if($Handoff){
            $disk=Wait-For {try{Owned-Disk}catch{return $null}}
            if($disk.PartitionStyle -ne 'GPT' -or $disk.Guid -ne $sourceGuid){throw 'Reconnected owned target GPT identity differs.'}
            if($NativeInitiator){$result.recoveryTargetOnline=Native-Initiator restore-target}else{$disk|Set-Disk -IsOffline $false}
            $partition=Mount-OwnedPartition $disk
        }
        $restarted=Control 0
        if(-not $restarted.recovered -or $restarted.cacheMappingsReused){throw 'Restart did not discard previous cache mappings.'}
        $result.recoveryStart=$restarted
        if($Handoff -and $NativeInitiator -and (-not $restarted.ssdPayloadProtected -or -not $restarted.ssdFileExclusive)){throw 'Native recovered payload did not retain its private exclusive lease.'}
        if($Handoff){
            $afterReuse=Get-Item -LiteralPath $cache
            $result.sameCacheAllocationReused=($afterReuse.CreationTimeUtc.ToFileTimeUtc() -eq $result.reusedCacheBefore.created -and $afterReuse.Length -eq $result.reusedCacheBefore.length)
            if(-not $result.sameCacheAllocationReused){throw 'Handoff recovery allocated a replacement SSD file.'}
        }
        Phase 'verify-native-reconnect-and-durable-data'
        $result.recoveryFixtureMap=Watch-Fixture recovered-fixture-extents -RetryTransient
        $result.recoveryCold=Client read-mutated recovered-cold -RetryTransient
        $recoveryCold=Control 0
        $result.recoveryWarm=Client read-mutated recovered-warm
        $recoveryWarm=Control 0
        $result.recoveryWarmDelta=Traffic-Delta $recoveryCold $recoveryWarm
        if($result.recoveryWarmDelta.hits -lt 96 -or $result.recoveryWarmDelta.ssdReadBytes -lt 6MB -or $result.recoveryWarmDelta.sourceReadBytes -ne 0){throw 'Recovered warm reads did not use SSD cache.'}
        $result.nativeReconnectPassed=$true
        if($Handoff){$result.recoveryFullOracle=Client read-all-mutated recovered-all-blocks}
    }
    $result.passed=$true
}catch{$result.error=$_.Exception.Message}finally{
    Phase 'cleanup'
    try {
        if($connected -or ((Get-Service MSiSCSI).Status -eq 'Running' -and @(Get-IscsiSession|Where-Object TargetNodeAddress -eq $iqn).Count)){
            $disk=Owned-Disk
            if($Handoff -and $NativeInitiator -and (Get-Partition -DriveLetter S -ErrorAction SilentlyContinue)){$result.nativeTargetOffline=Native-Initiator lock-target 'S:'}else{$disk|Set-Disk -IsOffline $true}
            if($NativeInitiator){$result.nativeDisconnect=Native-Initiator disconnect}else{$null=Disconnect-IscsiTarget -NodeAddress $iqn -Confirm:$false}
            $connected=$false
        }
        if($portalCreated){Remove-PrivateIscsiPortal;$portalCreated=$false}
        if($NativeInitiator -and (Get-Service MSiSCSI).Status -eq 'Running'){$result.nativeDisconnect=Native-Initiator disconnect}
        if($owned -and -not $owned.HasExited){$null=Control 5;if(-not $owned.WaitForExit(15000)){throw 'File-only target did not stop after disconnect.'}}
        if($Handoff -and $sourceCreated){
            if($owned -and -not $owned.HasExited){throw 'Raw source still owned; original was not remounted.'}
            $vhd=Get-VHD -Path $source
            if($vhd.Attached){
                $disk=Owned-Source
                if($NativeInitiator -and (Test-Path -LiteralPath (Join-Path $runRoot 'native-source.json'))){$result.nativeSourceOnline=Native-Initiator restore-source}else{$disk|Set-Disk -IsOffline $false}
                if($sourceFixtureReady){
                    $null=Mount-OwnedPartition (Owned-Source)
                    $restoreMode=if($mutationCompleted){'read-mutated'}else{'read'}
                    $result.restoredSourceOracle=Client $restoreMode restored-source
                    if((Get-Content -LiteralPath 'S:\preserved.txt' -Raw) -cne ('Existing file '+$run)){throw 'Other source file changed.'}
                    $result.originalSourceRestoredWithData=$true
                    Save
                }
                (Owned-Source)|Set-Disk -IsOffline $true
                if($previous.status -eq 'Stopped' -and (Get-Service MSiSCSI).Status -eq 'Running'){
                    if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Other initiator resources remain; preserving service.'}
                    Stop-Service MSiSCSI
                    Set-Service MSiSCSI -StartupType $previous.startType
                }
                $result.cleanupStep='detach-owned-vhdx';Save
                Dismount-VHD -Path $source
            }
        }
        if(($Handoff -and $sourceCreated -and -not (Get-VHD -Path $source).Attached) -or ($owned -and $owned.HasExited -and (Test-Path -LiteralPath (Join-Path $runRoot 'ready')))){
            if(Test-Path -LiteralPath $source){$result.sourceSha256=(Get-FileHash -LiteralPath $source).Hash}
            foreach($file in @($source,$cache,$recoveredCache)){if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}}
        }
    }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
    try {
        if($previous.status -eq 'Stopped' -and (Get-Service MSiSCSI).Status -eq 'Running'){
            if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Unexpected initiator user; preserving running service.'}
            Stop-Service MSiSCSI
        }
        Set-Service MSiSCSI -StartupType $previous.startType
    }catch{$result.initiatorRestoreError=$_.Exception.Message;$result.passed=$false}
    $result.sourceRemoved=-not(Test-Path -LiteralPath $source)
    $result.cacheRemoved=(-not(Test-Path -LiteralPath $cache) -and -not(Test-Path -LiteralPath $recoveredCache))
    $result.privateMountRemoved=-not(Test-Path -LiteralPath 'S:\fixture.bin')
    $current=Get-Service MSiSCSI
    $result.initiatorRestored=([string]$current.Status -eq $previous.status -and [string]$current.StartType -eq $previous.startType)
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootAfter=$after
    $result.bootUnchanged=($after.machineSha256 -eq $boot.machineSha256 -and $after.secureBootEnabled -eq $boot.secureBootEnabled -and $after.codeIntegrityOptions -eq $boot.codeIntegrityOptions)
    $result.parentsUnchanged=((Get-Partition -DriveLetter D|Get-Disk).UniqueId -eq $hdd.UniqueId -and (Get-Partition -DriveLetter M|Get-Disk).UniqueId -eq $ssd.UniqueId)
    $result.passed=[bool]($result.passed -and $result.sourceRemoved -and $result.cacheRemoved -and $result.privateMountRemoved -and $result.initiatorRestored -and $result.bootUnchanged -and $result.parentsUnchanged)
    if($Handoff){$result.passed=[bool]($result.passed -and $result.existingDataPreservedOnHandoff -and $result.originalSourceRestoredWithData -and -not $result.cacheSideFormattingPerformed)}
    $result.phase='finished';Save
    if($lock){$lock.Dispose()}
}
$result|ConvertTo-Json -Depth 12
if(-not $result.passed){throw 'Local iSCSI block-cache proof did not pass.'}
