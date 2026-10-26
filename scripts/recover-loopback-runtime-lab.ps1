# Recovery of one protected, disposable SCM test. Never accepts a physical disk.
param([Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Run)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
Start-Transcript -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-recovery.log') -Force|Out-Null
$config=Get-Content -LiteralPath 'C:\ProgramData\SuperHDDLoopback\manifest.json' -Raw|ConvertFrom-Json
if($config.run -ne $Run -or -not $config.lab -or $config.source.busType -ne 15 -or $config.source.bytes -ne 128MB){throw 'Exact disposable protected configuration required.'}
$path='D:\SuperHDD-handoff-'+$Run+'.vhdx'
$owner=Get-Content -LiteralPath (Join-Path $root ('artifacts\runtime\iscsi-local-'+$Run+'\handoff-source-owner.json')) -Raw|ConvertFrom-Json
$record=Get-Content -LiteralPath (Join-Path $root 'artifacts\loopback-runtime-lab.json') -Raw|ConvertFrom-Json
if($owner.runId -ne $Run -or $record.runId -ne $Run -or $owner.created -ne (Get-Item -LiteralPath $path).CreationTimeUtc.ToFileTimeUtc()){throw 'Original fixture file identity changed.'}
$vhd=Get-VHD -Path $path
if(-not $vhd.Attached -or $vhd.Size -ne 128MB -or $vhd.DiskNumber -ne $config.source.number){throw 'Owned VHDX dependency changed.'}
$disk=Get-Disk -Number $vhd.DiskNumber
if($disk.BusType -ne 'File Backed Virtual' -or $disk.IsBoot -or $disk.IsSystem){throw 'Only the disposable source is accepted.'}
$runRoot=Join-Path $root ('artifacts\runtime\iscsi-local-'+$Run)
$native=Join-Path $root 'artifacts\iscsi-probe\native-initiator.exe'
$sourceMarker=Join-Path $runRoot 'native-source.json'
if(-not(Test-Path -LiteralPath $sourceMarker)){$stream=[IO.File]::Open($sourceMarker,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read);try{$bytes=[Text.Encoding]::UTF8.GetBytes(($config.source|ConvertTo-Json -Depth 4));$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}}
$sessions=@(Get-IscsiSession|Where-Object TargetNodeAddress -eq ('iqn.2026-10.local.superhdd:cache-'+$Run))
if($sessions.Count -gt 1){throw 'Ambiguous owned sessions.'}
if($sessions.Count -eq 1){
    $targets=@($sessions[0]|Get-Disk)
    if($targets.Count -ne 1 -or $targets[0].BusType -ne 'iSCSI' -or $targets[0].SerialNumber.Trim() -ne $Run -or $targets[0].Guid -ne $disk.Guid -or $targets[0].Size -ne 128MB -or $targets[0].IsBoot -or $targets[0].IsSystem){throw 'Owned projection changed.'}
    if(-not $targets[0].IsOffline){$null=& $native lock-target-managed $Run 'S:';if($LASTEXITCODE){throw 'Could not safely lock the owned projected NTFS.'}}
}
$ready=Get-Content -LiteralPath (Join-Path 'C:\ProgramData\SuperHDDLoopback' ($Run+'\ready.json')) -Raw|ConvertFrom-Json
$worker=Get-Process -Id $ready.pid -ErrorAction SilentlyContinue
$workerOwned=$worker -and $worker.StartTime.ToUniversalTime().ToFileTimeUtc() -eq $ready.created
$state=Get-Content -LiteralPath (Join-Path 'C:\ProgramData\SuperHDDLoopback' ($Run+'\service-state.json')) -Raw|ConvertFrom-Json
$service=Get-CimInstance Win32_Service -Filter "Name='SuperHDDLoopbackLab'"
$installed=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'Super HDD Local Runtime'
if($service -and $service.State -ne 'Stopped'){
    $process=Get-Process -Id $service.ProcessId
    if($state.runId -ne $Run -or $state.servicePid -ne $process.Id -or $process.Path -ne (Join-Path $installed 'SuperHDDService.exe') -or $service.PathName -ne ('"'+(Join-Path $installed 'SuperHDDService.exe')+'" --loopback-lab-service')){throw 'SCM process identity changed; preserved.'}
    # The exact projected filesystem has been drained and is offline.
    $process.Kill();if(-not $process.WaitForExit(10000)){throw 'Stale supervisor did not exit.'}
}
if($service){& sc.exe delete SuperHDDLoopbackLab|Out-Null;if($LASTEXITCODE){throw 'Could not remove the owned lab service.'}}
if($sessions.Count){$null=& $native disconnect-managed $Run;if($LASTEXITCODE){throw 'Owned initiator logout failed.'}}
if($workerOwned){
    if($worker.Path -ne (Join-Path $installed 'SuperHDDCacheWorker.exe')){throw 'Worker executable changed.'}
    $token=[Guid]::NewGuid().ToString('N');Set-Content -LiteralPath (Join-Path 'C:\ProgramData\SuperHDDLoopback' ($Run+'\command')) -Value ($token+' 5') -Encoding ascii
    if(-not $worker.WaitForExit(20000)){throw 'Drained worker did not close; original source remains offline.'}
}
if((Get-Disk -Number $disk.Number).IsOffline){$null=& $native restore-source $Run;if($LASTEXITCODE){throw 'Native original source restoration failed.'}}
$part=@(Get-Partition -DiskNumber $disk.Number|Where-Object {$_.Offset -eq $config.source.partitionOffset -and $_.Size -eq $config.source.partitionBytes})
if($part.Count -ne 1){throw 'Original partition extent changed.'}
$part|Select-Object DiskNumber,PartitionNumber,DriveLetter,AccessPaths,IsOffline|ConvertTo-Json -Depth 3|Set-Content -LiteralPath (Join-Path $runRoot 'pre-recovery-partition.json') -Encoding utf8NoBOM
$null=& $native inspect $Run $config.sourceVolume.volumeGuid 2> (Join-Path $runRoot 'pre-recovery-source-inspection-error.log')
$current=Get-Partition -DriveLetter S -ErrorAction SilentlyContinue
if($current -and $current.DiskNumber -ne $disk.Number){throw 'S belongs to another disk; preserved.'}
if($part[0].DriveLetter -ne 'S'){$part[0]|Set-Partition -NewDriveLetter S}
$post=& $native inspect $Run 'S:' 2> (Join-Path $runRoot 'post-recovery-source-inspection-error.log');$post|Set-Content -LiteralPath (Join-Path $runRoot 'post-recovery-source-inspection.json') -Encoding utf8NoBOM
$end=[DateTime]::UtcNow.AddSeconds(20);while(-not(Test-Path 'S:\fixture.bin')){if([DateTime]::UtcNow -gt $end){throw 'Restored NTFS did not arrive.'};Start-Sleep -Milliseconds 100}
if($record.mutation){
    $client=Join-Path $root 'artifacts\iscsi-probe\iscsi-probe.exe';$report=Join-Path $runRoot 'recovery-full-original-oracle.json'
    $null=& $client client read-all-mutated 'S:\fixture.bin' $report
    if($LASTEXITCODE -or -not(Get-Content -LiteralPath $report -Raw|ConvertFrom-Json).passed){throw 'Modified original fixture oracle failed.'}
}elseif((Get-FileHash -LiteralPath 'S:\fixture.bin').Hash -ne $record.initialHash){throw 'Full original fixture hash failed.'}
if((Get-Content -LiteralPath 'S:\preserved.txt' -Raw) -cne ('Existing file '+$Run)){throw 'Unrelated fixture file changed.'}
$removed=& (Join-Path $installed 'SuperHDDService.exe') --remove-loopback-lab $Run
if($LASTEXITCODE){throw 'Protected owner removal failed.'}
(Get-Disk -Number $disk.Number)|Set-Disk -IsOffline $true
if(@(Get-IscsiSession).Count -or @(Get-IscsiTargetPortal).Count){throw 'Other initiator resources remain.'}
Stop-Service MSiSCSI;Set-Service MSiSCSI -StartupType Disabled
Dismount-VHD -Path $path
if((Get-VHD -Path $path).Attached){throw 'Owned VHDX remains attached.'}
Remove-Item -LiteralPath $path
[ordered]@{runId=$Run;passed=$true;baselineFixtureHash=$record.initialHash;mutationVerified=[bool]$record.mutation;fullModifiedOracle=$(if($record.mutation){Get-Content -LiteralPath (Join-Path $runRoot 'recovery-full-original-oracle.json') -Raw|ConvertFrom-Json}else{$null});sourceRestoredWithData=$true;sourceRemoved=(-not(Test-Path -LiteralPath $path));ownerRemoved=($removed|ConvertFrom-Json);scmRemoved=(-not(Get-Service SuperHDDLoopbackLab -ErrorAction SilentlyContinue));initiatorRestored=((Get-Service MSiSCSI).Status -eq 'Stopped' -and (Get-Service MSiSCSI).StartType -eq 'Disabled')}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $root ('artifacts\loopback-runtime-recovery-'+$Run+'.json')) -Encoding utf8NoBOM
