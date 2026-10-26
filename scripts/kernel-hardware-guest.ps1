param([ValidateSet('Prepare','Fixture','Baseline','Cold','Warm','Cleanup')][string]$Action,[string]$VmId,[int]$DiskNumber,[string]$DiskId,[string]$RunDirectory,[ValidateSet(1024,8192)][int]$FixtureBlocks=8192)
$ErrorActionPreference='Stop'
$lab='C:\SuperHDDLab'
$computer=Get-CimInstance Win32_ComputerSystem
if($computer.Manufacturer -ne 'Microsoft Corporation' -or $computer.Model -ne 'Virtual Machine' -or $env:COMPUTERNAME -ne 'SHDD-LAB' -or
    (Get-Content -LiteralPath (Join-Path $lab 'vm-identity.txt') -Raw).Trim() -ne $VmId){throw 'Disposable VM identity mismatch.'}
if($RunDirectory -notmatch '^C:\\SuperHDDLab\\hardware-run-[a-f0-9]{32}$'){throw 'Invalid owned benchmark report directory.'}
if(-not ('LabPnp' -as [type])){throw 'Load the validated lab helper types first.'}
$source='H:\source-hdd.vhdx'
$manifest=Join-Path $RunDirectory 'manifest.json'
function Native([string]$File,[string[]]$Arguments,[string]$Log){
    $saved=$ErrorActionPreference
    try{$ErrorActionPreference='Continue';$output=& $File @Arguments 2>&1;$code=$LASTEXITCODE}finally{$ErrorActionPreference=$saved}
    $output|Out-File -LiteralPath $Log -Encoding utf8
    if($code){throw "Lab tool failed ($code). Inspect $Log"}
}
if($Action -eq 'Prepare'){
    $disk=Get-Disk -Number $DiskNumber
    if($disk.UniqueId -ne $DiskId -or $disk.Size -ne 2GB -or $disk.BusType -notin 'SCSI','SAS' -or $disk.IsBoot -or $disk.IsSystem){throw ('New HDD-backed guest disk identity mismatch: '+($disk|Select-Object Number,UniqueId,Size,BusType,IsBoot,IsSystem|ConvertTo-Json -Compress))}
    if($disk.IsOffline){Set-Disk -Number $DiskNumber -IsOffline $false}
    if($disk.PartitionStyle -eq 'RAW'){
        if(Get-Volume -DriveLetter H -ErrorAction SilentlyContinue){throw 'Guest drive H is already in use.'}
        Initialize-Disk -Number $DiskNumber -PartitionStyle MBR|Out-Null
        New-Partition -DiskNumber $DiskNumber -UseMaximumSize -DriveLetter H|Format-Volume -FileSystem NTFS -NewFileSystemLabel SHDD-HDD-Lab -Confirm:$false|Out-Null
    }elseif((Get-Partition -DriveLetter H|Get-Disk).UniqueId -ne $DiskId -or (Get-Volume -DriveLetter H).FileSystemLabel -ne 'SHDD-HDD-Lab'){
        throw 'Refusing to reuse an unrelated guest partition.'
    }
    Native (Join-Path $lab 'raw_lab.exe') @('disable') (Join-Path $lab 'hdd-disable.log')
    if((Get-Volume -DriveLetter R).FileSystemLabel -ne 'SHDD-HDD-Source'){
        [LabPnp]::Remove([LabVolume]::Device('R:'))
        @'
select vdisk file="C:\SuperHDDLab\source.vhdx"
detach vdisk
exit
'@|Set-Content -LiteralPath (Join-Path $lab 'hdd-detach-old.txt')
        Native diskpart.exe @('/s',(Join-Path $lab 'hdd-detach-old.txt')) (Join-Path $lab 'hdd-detach-old.log')
        if(Get-Volume -DriveLetter R -ErrorAction SilentlyContinue){throw 'Old source still owns drive R.'}
        if(Test-Path -LiteralPath $source){
            @'
select vdisk file="H:\source-hdd.vhdx"
attach vdisk
exit
'@|Set-Content -LiteralPath (Join-Path $lab 'hdd-source-reattach.txt')
            Native diskpart.exe @('/s',(Join-Path $lab 'hdd-source-reattach.txt')) (Join-Path $lab 'hdd-source-reattach.log')
            if((Get-Volume -DriveLetter R).FileSystemLabel -ne 'SHDD-HDD-Source'){throw 'Existing guest HDD fixture identity mismatch.'}
        }else{
        @'
create vdisk file="H:\source-hdd.vhdx" maximum=1024 type=expandable
select vdisk file="H:\source-hdd.vhdx"
attach vdisk
create partition primary
format fs=ntfs quick label=SHDD-HDD-Source
assign letter=R
exit
'@|Set-Content -LiteralPath (Join-Path $lab 'hdd-source-create.txt')
        Native diskpart.exe @('/s',(Join-Path $lab 'hdd-source-create.txt')) (Join-Path $lab 'hdd-source-create.log')
        }
    }
    $sourceDisk=Get-Partition -DriveLetter R|Get-Disk
    $cacheDisk=Get-Partition -DriveLetter T|Get-Disk
    if($sourceDisk.BusType -ne 'File Backed Virtual' -or $sourceDisk.IsBoot -or $sourceDisk.IsSystem -or $sourceDisk.Number -eq $cacheDisk.Number){throw 'Invalid nested disposable source/cache identity.'}
    [LabVolume]::ResetOwned('\\.\T:',[IO.File]::ReadAllBytes((Join-Path $lab 'marker.bin')))
    [LabPnp]::Enroll([LabVolume]::Device('R:'))
    $node=Get-PnpDevice -Class Volume -PresentOnly|Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_PDOName).Data -eq [LabVolume]::Device('R:')}
    if(@($node).Count -ne 1){throw 'Cannot identify the new source PDO.'}
    Native pnputil.exe @('/restart-device',$node.InstanceId) (Join-Path $lab 'hdd-source-restart.log')
    $request=[ordered]@{sourceDevice=[LabVolume]::Device('R:');cacheDevice=[LabVolume]::Device('T:');capacityBytes=768MB;acknowledgedDisposableVhds=$true}
    New-Item -ItemType Directory -Path $RunDirectory -Force|Out-Null
    $request|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $RunDirectory 'request.json') -Encoding ASCII
    [pscustomobject]@{sourceContainer=$source;sourceDisk=$sourceDisk.Number;cacheContainer='C:\SuperHDDLab\cache.vhdx';cacheDisk=$cacheDisk.Number;hddBackingDisk=$DiskNumber;hddBackingUniqueId=$DiskId;cacheBudgetBytes=768MB}
    return
}
if($Action -eq 'Fixture'){
    if(Test-Path -LiteralPath $manifest){throw 'A benchmark fixture already exists for this run.'}
    $fixture='R:\superhdd-kernel-bench-'+[Guid]::NewGuid().ToString('N')
    for($attempt=0;$attempt -lt 20;$attempt++){
        try{
            Native (Join-Path $lab 'raw_benchmark.exe') @('prepare',(Join-Path $RunDirectory 'request.json'),$fixture,$manifest,[string]$FixtureBlocks) (Join-Path $RunDirectory 'prepare.log')
            break
        }catch{
            $message=Get-Content -LiteralPath (Join-Path $RunDirectory 'prepare.log') -Raw
            if($attempt -eq 19 -or $message -notmatch 'os error 170' -or $message -notmatch 'stage: 14' -or (Test-Path -LiteralPath $fixture)){throw}
            Start-Sleep -Seconds 1
        }
    }
    [pscustomobject]@{prepared=$true;fixture=$fixture;manifest=$manifest};return
}
if($Action -in 'Baseline','Cold','Warm'){
    $mode=$Action.ToLowerInvariant();$report=Join-Path $RunDirectory ($mode+'.json')
    Native (Join-Path $lab 'raw_benchmark.exe') @('read',$manifest,$mode,$report) (Join-Path $RunDirectory ($mode+'.log'))
    Get-Content -LiteralPath $report -Raw|ConvertFrom-Json;return
}
if($Action -eq 'Cleanup'){
    Native (Join-Path $lab 'raw_benchmark.exe') @('cleanup',$manifest) (Join-Path $RunDirectory 'cleanup.log')
    [LabPnp]::Remove([LabVolume]::Device('R:'))
    @'
select vdisk file="H:\source-hdd.vhdx"
detach vdisk
exit
'@|Set-Content -LiteralPath (Join-Path $RunDirectory 'detach.txt')
    Native diskpart.exe @('/s',(Join-Path $RunDirectory 'detach.txt')) (Join-Path $RunDirectory 'detach.log')
    [LabVolume]::ResetOwned('\\.\T:',[IO.File]::ReadAllBytes((Join-Path $lab 'marker.bin')))
    $disk=Get-Disk -Number $DiskNumber
    if($disk.UniqueId -ne $DiskId -or $disk.IsBoot -or $disk.IsSystem){throw 'Guest cleanup disk identity changed.'}
    Set-Disk -Number $DiskNumber -IsOffline $true
    [pscustomobject]@{fixtureRemoved=$true;cacheResetToOwnedBootstrap=$true;hddGuestDiskOffline=$true}
}
