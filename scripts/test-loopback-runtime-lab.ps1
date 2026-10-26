# Candidate permanent-worker protocol exercised only against a new 128 MiB VHDX.
param([switch]$VerifyReservation,[switch]$Scm)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$run=[Guid]::NewGuid().ToString('N')
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$run)
$control=Join-Path 'C:\ProgramData\SuperHDDLoopback' $run
$record=Join-Path $root 'artifacts\loopback-runtime-lab.json'
$source='D:\SuperHDD-handoff-'+$run+'.vhdx'
$native=Join-Path $root 'artifacts\iscsi-probe\native-initiator.exe'
$client=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe'
$worker=Join-Path $root 'artifacts\loopback-runtime\SuperHDDCacheWorker.exe'
$serviceExe=Join-Path $root 'target\loopback-service\release\SuperHDDService.exe'
$serviceName='SuperHDDLoopbackLab'
if($Scm){
    $package=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'Super HDD Local Runtime'
    $serviceExe=Join-Path $package 'SuperHDDService.exe';$worker=Join-Path $package 'SuperHDDCacheWorker.exe'
    if(-not(Test-Path -LiteralPath (Join-Path $package 'native-package.json'))){throw 'Protected package is required for SCM validation.'}
    if(Get-Service $serviceName -ErrorAction SilentlyContinue){throw 'An existing SCM lab service is preserved.'}
}
$iqn='iqn.2026-10.local.superhdd:cache-'+$run
if(Get-PSDrive S -ErrorAction SilentlyContinue){throw 'S is already in use.'}
if(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json'){throw 'Existing protected runtime owner must be recovered first.'}
if(Get-NetTCPConnection -State Listen -LocalPort 33262 -ErrorAction SilentlyContinue){throw 'Private target port is in use.'}
$sourceParent=Get-Partition -DriveLetter D|Get-Disk;$cacheParent=Get-Partition -DriveLetter M|Get-Disk
if($sourceParent.IsBoot -or $sourceParent.IsSystem -or $sourceParent.FriendlyName -notmatch 'HUA722020ALA331' -or $cacheParent.FriendlyName -notmatch '980 PRO' -or $sourceParent.Number -eq $cacheParent.Number){throw 'Expected separate D/M parents absent.'}
$previous=Get-Service MSiSCSI
$before=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
$null=New-Item -ItemType Directory -Path $runRoot
$result=[ordered]@{runId=$run;passed=$false;phase='starting';error=$null;scope='Candidate protected runtime worker with disposable VHDX only';physicalDAccelerationActive=$false}
$owned=$null;$created=$false;$prepared=$false;$connected=$false;$sourceReady=$false;$mutated=$false;$partition=$null;$sourceGuid=$null;$sourceCreation=$null;$lock=$null
$serviceCreated=$false
function Save {$result|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $record -Encoding utf8NoBOM}
function Step([string]$phase){$result.phase=$phase;Save}
function Wait-Owned([scriptblock]$action,[int]$seconds=30){$end=[DateTime]::UtcNow.AddSeconds($seconds);do{$value=& $action;if($value){return $value};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $end);throw 'Owned runtime wait timed out.'}
function Native([string]$operation,[string]$volume){
    $arguments=@($operation,$run);if($volume){$arguments+=$volume}
    $json=& $native @arguments 2> (Join-Path $runRoot ('runtime-native-'+$operation+'-error.log'))
    if($LASTEXITCODE){throw ('Owned native operation failed: '+$operation)};$json|ConvertFrom-Json
}
function Service-Lab([string]$operation){$json=& $serviceExe $operation $run 2> (Join-Path $runRoot ('service-'+$operation+'-error.log'));if($LASTEXITCODE){throw ('Protected fixture owner operation failed: '+$operation)};$json|ConvertFrom-Json}
function Source-Disk {
    $vhd=Get-VHD -Path $source
    if(-not $created -or -not $vhd.Attached -or $vhd.Size -ne 128MB -or (Get-Item -LiteralPath $source).CreationTimeUtc.ToFileTimeUtc() -ne $sourceCreation){throw 'Owned VHDX dependency changed.'}
    $disk=Get-Disk -Number $vhd.DiskNumber
    if($disk.BusType -ne 'File Backed Virtual' -or $disk.Size -ne 128MB -or $disk.IsBoot -or $disk.IsSystem -or $disk.Number -in @($sourceParent.Number,$cacheParent.Number) -or ($sourceGuid -and $disk.Guid -ne $sourceGuid)){throw 'Only the exact disposable source is accepted.'};$disk
}
function Target-Disk {
    $sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq $iqn);if($sessions.Count -ne 1){throw 'Require one owned managed session.'}
    $disks=@($sessions|Get-Disk)
    if($disks.Count -ne 1 -or $disks[0].BusType -ne 'iSCSI' -or $disks[0].SerialNumber.Trim() -ne $run -or $disks[0].Size -ne 128MB -or $disks[0].Guid -ne $sourceGuid -or $disks[0].IsBoot -or $disks[0].IsSystem){throw 'Managed projection identity changed.'};$disks[0]
}
function Bind-OwnedPartition($disk){
    $parts=@(Get-Partition -DiskNumber $disk.Number|Where-Object {$_.Guid -eq $partition.Guid -and $_.Offset -eq $partition.Offset -and $_.Size -eq $partition.Size})
    if($parts.Count -ne 1){throw 'Owned data partition changed.'}
    $old=Get-Partition -DriveLetter S -ErrorAction SilentlyContinue
    if($old -and ($old.DiskNumber -ne $disk.Number -or $old.Guid -ne $partition.Guid)){throw 'S belongs to another disk.'}
    if($parts[0].DriveLetter -ne 'S'){$parts[0]|Set-Partition -NewDriveLetter S}
    $null=Wait-Owned {Test-Path -LiteralPath 'S:\fixture.bin'}
}
function Client([string]$mode,[string]$tag){
    $report=Join-Path $runRoot ($tag+'.json')
    $process=Start-Process -FilePath $client -ArgumentList @('client',$mode,'S:\fixture.bin',('"'+$report+'"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runRoot ($tag+'.log')) -RedirectStandardError (Join-Path $runRoot ($tag+'-error.log'))
    if(-not $process.WaitForExit(60000)){$process.Kill();throw 'Owned native fixture read timed out.'}
    if($process.ExitCode){throw ('Owned fixture oracle failed: '+$tag)}
    Get-Content -LiteralPath $report -Raw|ConvertFrom-Json
}
function Control([int]$operation){
    if($Scm -and $operation -eq 0){
        $statsPath=Join-Path $control 'stats.json';$before=(Get-Item -LiteralPath $statsPath).LastWriteTimeUtc
        & sc.exe control $serviceName 132|Out-Null;if($LASTEXITCODE){throw 'SCM snapshot request failed.'}
        $null=Wait-Owned {(Get-Item -LiteralPath $statsPath).LastWriteTimeUtc -gt $before} 15
        return (Wait-Owned {try{$stream=[IO.File]::Open($statsPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete));try{$reader=[IO.StreamReader]::new($stream);$reader.ReadToEnd()|ConvertFrom-Json}finally{$stream.Dispose()}}catch{return $null}})
    }
    $token=[Guid]::NewGuid().ToString('N');Set-Content -LiteralPath (Join-Path $control 'command') -Value ($token+' '+$operation) -Encoding ascii
    $null=Wait-Owned {$ack=Join-Path $control 'ack';if(Test-Path -LiteralPath $ack){(Get-Content -LiteralPath $ack -Raw).Trim() -eq ($token+' 0')}} 20
    if($operation -ne 5){Get-Content -LiteralPath (Join-Path $control 'stats.json') -Raw|ConvertFrom-Json}
}
function Start-OwnedWorker([string]$tag){
    $ready=Join-Path $control 'ready.json';if(Test-Path -LiteralPath $ready){Remove-Item -LiteralPath $ready}
    $script:owned=Start-Process -FilePath $worker -ArgumentList @($run) -WorkingDirectory (Split-Path $worker) -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runRoot ($tag+'.log')) -RedirectStandardError (Join-Path $runRoot ($tag+'-error.log'))
    $null=Wait-Owned {if($owned.HasExited){throw 'Candidate runtime worker exited.'};Test-Path -LiteralPath $ready}
    $marker=Get-Content -LiteralPath $ready -Raw|ConvertFrom-Json
    if($marker.runId -ne $run -or $marker.pid -ne $owned.Id -or $marker.sourceBytes -ne 128MB){throw 'Unexpected protected worker identity.'}
}
function Read-ScmState {
    # The protected writer keeps DELETE access for owned cleanup. Match its
    # sharing contract and retry incomplete snapshots while it publishes.
    try{
        $stream=[IO.File]::Open((Join-Path $control 'service-state.json'),[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        try{$reader=[IO.StreamReader]::new($stream);$text=$reader.ReadToEnd();if($text.Length -gt 32768){throw 'Oversized SCM state.'};$value=$text|ConvertFrom-Json;if($value.runId -eq $run -and $value.schema -eq 1){$value}}finally{$stream.Dispose()}
    }catch{return $null}
}
function Adopt-ScmWorker([int]$previousPid=0){
    $script:owned=Wait-Owned {
        try{
            $marker=Get-Content -LiteralPath (Join-Path $control 'ready.json') -Raw|ConvertFrom-Json
            $state=Read-ScmState
            if($marker.runId -ne $run -or $marker.sourceBytes -ne 128MB -or $marker.pid -eq $previousPid -or $state.runId -ne $run -or $state.phase -ne 'active'){return $null}
            $process=Get-Process -Id $marker.pid -ErrorAction Stop
            if($process.Path -ne $worker -or $process.StartTime.ToUniversalTime().ToFileTimeUtc() -ne $marker.created){throw 'SCM worker process identity changed.'}
            $process
        }catch{return $null}
    } 80
}
function Public-Status {
    $text=& $serviceExe --loopback-lab-status 2> (Join-Path $runRoot 'public-status-error.log')
    if($LASTEXITCODE){throw 'Authenticated read-only status unavailable.'}
    $value=$text|ConvertFrom-Json
    if($value.nativeVersion -ne 6 -or -not $value.labOnly -or $value.sourceVolume -ne 'S:'){throw 'Read-only status returned a different role or protocol.'}
    $value
}
function Scm-Action([int]$code,[string]$phase){
    & sc.exe control $serviceName $code|Out-Null;if($LASTEXITCODE){throw 'Owned SCM action request failed.'}
    Wait-Owned {try{$value=Public-Status;if($value.phase -eq $phase){$value}}catch{return $null}} 15
}
try{
    $lock=[IO.File]::Open((Join-Path $root 'artifacts\runtime\kernel-lab\hardware-benchmark.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    Step 'create-disposable-existing-data-source'
    $null=New-VHD -Path $source -Fixed -SizeBytes 128MB -LogicalSectorSizeBytes 512 -PhysicalSectorSizeBytes 512;$created=$true
    $sourceCreation=(Get-Item -LiteralPath $source).CreationTimeUtc.ToFileTimeUtc()
    $null=Mount-VHD -Path $source -NoDriveLetter;$disk=Source-Disk;$disk|Set-Disk -IsOffline $false;$disk|Set-Disk -IsReadOnly $false
    $null=Initialize-Disk -Number $disk.Number -PartitionStyle GPT;$sourceGuid=(Get-Disk -Number $disk.Number).Guid
    $partition=New-Partition -DiskNumber $disk.Number -UseMaximumSize -DriveLetter S
    $null=Format-Volume -DriveLetter S -FileSystem NTFS -AllocationUnitSize 4096 -NewFileSystemLabel 'SuperHDD-RuntimeLab' -Force -Confirm:$false
    $result.seed=Client seed seed;[IO.File]::WriteAllText('S:\preserved.txt',('Existing file '+$run));$sourceReady=$true
    $result.initialHash=(Get-FileHash -LiteralPath 'S:\fixture.bin').Hash
    [ordered]@{runId=$run;created=$sourceCreation;sourceBytes=128MB}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $runRoot 'handoff-source-owner.json') -Encoding utf8NoBOM
    Step 'prepare-private-journal-before-source-lock'
    if($Scm){$result.packageBinaries=(Get-Content -LiteralPath (Join-Path $package 'native-package.json') -Raw|ConvertFrom-Json).files}
    $result.prepared=Service-Lab '--prepare-loopback-lab';$prepared=$true
    if($previous.StartType -eq 'Disabled'){Set-Service MSiSCSI -StartupType Manual};Start-Service MSiSCSI
    if($VerifyReservation){
        Step 'verify-reserved-port-before-listener'
        Set-Content -LiteralPath (Join-Path $runRoot 'port-reservation-ready') -Value $run -Encoding ascii
        $null=Wait-Owned {Test-Path -LiteralPath (Join-Path $runRoot 'port-reservation-verified.json')} 120
        $result.nonAdminEndpointProbe=Get-Content -LiteralPath (Join-Path $runRoot 'port-reservation-verified.json') -Raw|ConvertFrom-Json
        if(-not $result.nonAdminEndpointProbe.passed){throw 'Unprivileged endpoint isolation was not verified.'}
    }
    if($Scm){
        Step 'start-protected-scm-owner'
        $null=New-Service -Name $serviceName -BinaryPathName ('"'+$serviceExe+'" --loopback-lab-service') -DisplayName 'Super HDD disposable cache service test' -StartupType Manual -DependsOn MSiSCSI
        $serviceCreated=$true;Start-Service $serviceName;Adopt-ScmWorker
        $connected=$true;$result.scmOwnerStarted=$true
    }else{
        $result.sourceOffline=Native lock-source 'S:'
        Step 'start-protected-runtime-worker'
        Start-OwnedWorker initial-worker
    }
    $result.initialStats=Control 0
    if(-not $Scm){$result.connected=Native connect-managed;$connected=$true}
    $null=Wait-Owned {try{Target-Disk}catch{return $null}}
    if(-not $Scm){$null=Native restore-target-managed;Bind-OwnedPartition (Target-Disk)}else{$null=Wait-Owned {Test-Path -LiteralPath 'S:\fixture.bin'}}
    if((Get-FileHash -LiteralPath 'S:\fixture.bin').Hash -ne $result.initialHash){throw 'Existing data changed during managed handoff.'}
    $result.existingDataPreserved=$true
    $result.cold=Client read cold;$cold=Control 0
    $result.warm=Client read warm;$warm=Control 0
    if($warm.statistics.hits -le $cold.statistics.hits -or $warm.statistics.ssdReadBytes - $cold.statistics.ssdReadBytes -lt 6MB){throw 'Managed worker did not use the actual SSD payload.'}
    $result.warmDelta=[ordered]@{hits=$warm.statistics.hits-$cold.statistics.hits;ssdReadBytes=$warm.statistics.ssdReadBytes-$cold.statistics.ssdReadBytes;sourceReadBytes=$warm.statistics.sourceReadBytes-$cold.statistics.sourceReadBytes}
    $result.mutation=Client mutate mutation;$mutated=$true
    if($Scm){$result.authenticatedPublicStatus=Scm-Action 129 paused;$result.publicReadOnlyStatusVerified=$true}
    Step 'interrupt-and-recover-managed-worker'
    $oldPid=$owned.Id;$owned.Kill();if(-not $owned.WaitForExit(5000)){throw 'Owned worker did not terminate.'}
    if($Scm){Adopt-ScmWorker $oldPid;$result.scmAutomaticallyRecoveredWorker=$true}else{Start-OwnedWorker recovered-worker}
    $result.recoveryStart=Control 0
    $null=Wait-Owned {try{Target-Disk}catch{return $null}}
    if(-not $Scm){$null=Native restore-target-managed;Bind-OwnedPartition (Target-Disk)}else{$null=Wait-Owned {Test-Path -LiteralPath 'S:\fixture.bin'}}
    $result.recoveryFullOracle=Client read-all-mutated recovered-full
    $result.recoveryStats=Control 0
    if(-not $result.recoveryStats.ssdPayloadProtected -or -not $result.recoveryStats.ssdFileExclusive -or $result.recoveryStats.cacheMappingsReused){throw 'Recovered worker did not retain the expected protected fresh cache lease.'}
    if($Scm){
        if(-not $result.recoveryStats.paused){throw 'Paused intent was lost across worker restart.'};$result.pausePersistedAfterWorkerRestart=$true
        Step 'restart-scm-owner-and-adopt-existing-worker'
        $state=Wait-Owned {Read-ScmState}
        $serviceInfo=Get-CimInstance Win32_Service -Filter "Name='SuperHDDLoopbackLab'"
        $ownerProcess=Get-Process -Id $state.servicePid
        if($state.runId -ne $run -or $serviceInfo.ProcessId -ne $ownerProcess.Id -or $ownerProcess.Path -ne $serviceExe){throw 'SCM supervisor identity changed.'}
        $adoptedPid=$owned.Id;$ownerProcess.Kill();if(-not $ownerProcess.WaitForExit(5000)){throw 'Owned supervisor did not exit.'}
        $null=Wait-Owned {(Get-Service $serviceName).Status -eq 'Stopped'} 15
        if($owned.HasExited){throw 'Worker did not survive its supervisor.'}
        $result.orphanWorkerFullOracle=Client read-all-mutated owner-interrupted-full
        Start-Service $serviceName;Adopt-ScmWorker
        if($owned.Id -ne $adoptedPid){throw 'SCM restart did not adopt its pinned existing worker.'}
        $result.scmExistingWorkerAdopted=$true
        if((Public-Status).phase -ne 'paused'){throw 'Paused state was lost during supervisor adoption.'}
        $result.resumedPublicStatus=Scm-Action 130 active
        Step 'scm-refuses-stop-with-open-file'
        $busy=[IO.File]::Open('S:\fixture.bin',[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        try{
            & sc.exe control $serviceName 131|Out-Null;if($LASTEXITCODE){throw 'SCM drain request failed.'}
            $null=Wait-Owned {(Read-ScmState).phase -eq 'attention'} 15
            if((Get-Service $serviceName).Status -ne 'Running' -or $owned.HasExited -or (Source-Disk).IsOffline -ne $true){throw 'Busy SCM stop did not preserve the running export.'}
            $result.scmBusyStopRefused=$true
        }finally{$busy.Dispose()}
    }
    $result.passed=$true
}catch{$result.error=$_.Exception.Message;$result.errorLocation=$_.ScriptStackTrace}finally{
    Step 'cleanup'
    try{
        if($Scm -and $serviceCreated){
            if((Get-Service $serviceName).Status -ne 'Stopped'){& sc.exe control $serviceName 131|Out-Null;if($LASTEXITCODE){throw 'Owned SCM drain request failed.'}}
            $null=Wait-Owned {(Get-Service $serviceName).Status -eq 'Stopped'} 45
            $null=Wait-Owned {-not(Get-Process -Name SuperHDDCacheWorker -ErrorAction SilentlyContinue)} 15
            & sc.exe delete $serviceName|Out-Null;if($LASTEXITCODE){throw 'Owned SCM service removal failed.'}
            $serviceCreated=$false;$connected=$false;$owned=$null;$result.scmStoppedAndRemoved=$true
        }
        if($connected){$disk=Target-Disk;if(-not $disk.IsOffline){$null=Native lock-target-managed 'S:'};$result.disconnected=Native disconnect-managed;$connected=$false}
        if($owned -and -not $owned.HasExited){$null=Control 5;if(-not $owned.WaitForExit(15000)){throw 'Drained worker did not close.'}}
        if($created -and (Get-VHD -Path $source).Attached){
            $disk=Source-Disk
            if(Test-Path -LiteralPath (Join-Path $runRoot 'native-source.json')){$null=Native restore-source}else{$disk|Set-Disk -IsOffline $false}
            if($sourceReady){Bind-OwnedPartition (Source-Disk);$mode=if($mutated){'read-all-mutated'}else{'read'};$result.restoredSourceOracle=Client $mode original-source;if((Get-Content -LiteralPath 'S:\preserved.txt' -Raw) -cne ('Existing file '+$run)){throw 'Other original source data changed.'};$result.sourceRestoredWithData=$true;Save}
            if($prepared){$result.ownerRemoved=Service-Lab '--remove-loopback-lab';$prepared=$false}
            (Source-Disk)|Set-Disk -IsOffline $true
            if($previous.Status -eq 'Stopped' -and -not @(Get-IscsiSession).Count -and -not @(Get-IscsiTargetPortal).Count){Stop-Service MSiSCSI;Set-Service MSiSCSI -StartupType $previous.StartType}
            Step 'detach-owned-vhdx';Dismount-VHD -Path $source
        }
        if($created -and -not (Get-VHD -Path $source).Attached){Remove-Item -LiteralPath $source}
    }catch{$result.cleanupError=$_.Exception.Message;$result.passed=$false}
    try{if($previous.Status -eq 'Stopped' -and (Get-Service MSiSCSI).Status -eq 'Running'){if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Other initiator resources remain.'};Stop-Service MSiSCSI};Set-Service MSiSCSI -StartupType $previous.StartType}catch{$result.initiatorRestoreError=$_.Exception.Message;$result.passed=$false}
    $result.sourceRemoved=-not(Test-Path -LiteralPath $source);$result.ownerManifestRemoved=-not(Test-Path -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json')
    $after=& (Join-Path $PSScriptRoot 'local-boot-state.ps1')
    $result.bootUnchanged=($before.machineSha256 -eq $after.machineSha256 -and $before.codeIntegrityOptions -eq $after.codeIntegrityOptions -and $before.secureBootEnabled -eq $after.secureBootEnabled)
    $result.parentsUnchanged=((Get-Partition -DriveLetter D|Get-Disk).UniqueId -eq $sourceParent.UniqueId -and (Get-Partition -DriveLetter M|Get-Disk).UniqueId -eq $cacheParent.UniqueId)
    $current=Get-Service MSiSCSI;$result.initiatorRestored=($current.Status -eq $previous.Status -and $current.StartType -eq $previous.StartType)
    $result.passed=[bool]($result.passed -and $result.sourceRemoved -and $result.ownerManifestRemoved -and $result.sourceRestoredWithData -and $result.bootUnchanged -and $result.parentsUnchanged -and $result.initiatorRestored)
    $result.phase='finished';Save;if($lock){$lock.Dispose()}
}
if(-not $result.passed){throw 'Protected permanent-worker lab did not pass.'}
